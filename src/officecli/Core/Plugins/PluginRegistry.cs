// Copyright 2026 OfficeCLI (https://OfficeCLI.AI)
// SPDX-License-Identifier: Apache-2.0

using System.Collections;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using OfficeCli.Core;

namespace OfficeCli.Core.Plugins;

/// <summary>
/// A plugin executable resolved on disk together with its parsed manifest.
/// </summary>
public sealed record ResolvedPlugin(string ExecutablePath, PluginManifest Manifest);

/// <summary>
/// Locates plugin executables and reads their manifests. Implements the
/// 4-path discovery rules in plugins/plugin-protocol.md §3.
///
/// Lookup is cached for the process lifetime. Negative results are cached too,
/// so a missing plugin is not re-probed on every operation.
/// </summary>
public static class PluginRegistry
{
    private const int InfoTimeoutMs = 5000;
    private const int EnumerationProbeBudgetMs = 30000;
    private const int MaxDiscoveryCandidates = 256;
    private const int MaxManagedDirectoryEntriesScanned = 4096;
    private const int MaxPathEntriesScanned = 4096;
    private const int ManifestOutputByteLimit = 1024 * 1024;
    private const int AggregateManifestOutputByteLimit = 16 * 1024 * 1024;
    private const int ManifestDiagnosticRetainedByteLimit = 16 * 1024;

    private static readonly Encoding StrictUtf8 = new UTF8Encoding(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true);

    private static StringComparer PathComparer =>
        OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;

    private static readonly Dictionary<(PluginKind kind, string ext), ResolvedPlugin?> _cache
        = new();
    private static readonly object _cacheLock = new();
    private static readonly ConditionalWeakTable<ResolvedPlugin, ManifestMetadata> _manifestIdentities
        = new();

    /// <summary>
    /// Resolve a plugin for the given kind + file extension. Returns null if no
    /// plugin is installed at any discovery path, or if the plugin failed
    /// <c>--info</c> probing.
    /// </summary>
    /// <param name="kind">Plugin kind we're looking for.</param>
    /// <param name="ext">File extension with leading dot, lowercase (e.g. ".doc").</param>
    public static ResolvedPlugin? FindFor(PluginKind kind, string ext)
    {
        ext = NormalizeExt(ext);
        var key = (kind, ext);

        lock (_cacheLock)
            if (_cache.TryGetValue(key, out var hit))
                return hit;

        var resolved = ResolveUncached(kind, ext);

        lock (_cacheLock)
            _cache[key] = resolved;
        return resolved;
    }

    /// <summary>
    /// Clear the resolution cache. Useful for `officecli plugins install` to
    /// force re-discovery without restarting the process. Not thread-safe with
    /// concurrent <see cref="FindFor"/> calls; callers must quiesce first.
    /// </summary>
    public static void InvalidateCache()
    {
        lock (_cacheLock) _cache.Clear();
    }

    /// <summary>
    /// Enumerate every plugin discoverable on this machine (across all kinds /
    /// extensions). Used by `officecli plugins list`. Each result reports its
    /// own kinds/extensions from the manifest.
    /// </summary>
    public static IReadOnlyList<ResolvedPlugin> EnumerateAll() =>
        EnumerateAll(EnumerationProbeBudgetMs);

    internal static IReadOnlyList<ResolvedPlugin> EnumerateAll(int probeBudgetMs) =>
        ProbeCandidates(MaterializeDiscoveryCandidates(), probeBudgetMs);

    internal static IReadOnlyList<ResolvedPlugin> ProbeCandidates(
        IReadOnlyList<string> candidates,
        int probeBudgetMs)
    {
        var seen = new Dictionary<string, ResolvedPlugin>(PathComparer);
        var acceptedManifestBytes = 0;
        var timer = Stopwatch.StartNew();

        foreach (var normalized in candidates)
        {
            var remainingMs = probeBudgetMs - (int)timer.ElapsedMilliseconds;
            if (remainingMs <= 0) throw DiscoveryTimeout(probeBudgetMs);
            if (TryReadManifest(
                normalized,
                out var manifest,
                out var identity,
                out _,
                out var manifestOutputBytes,
                Math.Min(InfoTimeoutMs, remainingMs)))
            {
                if (manifestOutputBytes > AggregateManifestOutputByteLimit - acceptedManifestBytes)
                {
                    throw DiscoveryLimit(
                        $"accepted manifests exceed the {AggregateManifestOutputByteLimit / (1024 * 1024)} MiB aggregate output budget",
                        "Reduce oversized plugin manifest metadata, remove unnecessary registrations, or query a verified plugin by absolute executable path.");
                }
                acceptedManifestBytes += manifestOutputBytes;
                seen[normalized] = CreateResolvedPlugin(normalized, manifest, identity);
            }

            // A timeout at the global boundary invalidates the entire snapshot.
            // Never return a partial registration list whose conflicts or higher
            // priority aliases may simply have been left unprobed.
            if (timer.ElapsedMilliseconds >= probeBudgetMs) throw DiscoveryTimeout(probeBudgetMs);
        }

        return seen.Values.ToList();
    }

    // ---------------------------------------------------------------------
    // Discovery
    // ---------------------------------------------------------------------

    private static ResolvedPlugin? ResolveUncached(PluginKind kind, string ext)
    {
        foreach (var candidate in CandidatePaths(kind, ext))
        {
            if (!File.Exists(candidate)) continue;
            if (!TryReadManifest(candidate, out var m, out var identity)) continue;
            if (!ManifestMatches(m, kind, ext)) continue;
            return CreateResolvedPlugin(candidate, m, identity);
        }
        return null;
    }

    /// <summary>
    /// The 4 discovery paths in priority order. Yields candidate executable
    /// paths; the caller is responsible for File.Exists checks.
    /// </summary>
    private static IEnumerable<string> CandidatePaths(PluginKind kind, string ext)
    {
        var kindWire = kind.ToWireString();
        var extBare = ext.TrimStart('.');

        // 1. Environment variable: $OFFICECLI_PLUGIN_<KIND>_<EXT>
        var envName = $"OFFICECLI_PLUGIN_{kindWire.ToUpperInvariant().Replace('-', '_')}_{extBare.ToUpperInvariant()}";
        var envValue = Environment.GetEnvironmentVariable(envName);
        if (!string.IsNullOrWhiteSpace(envValue) &&
            TryNormalizeFullyQualifiedPath(envValue, out var normalizedEnvValue))
        {
            yield return normalizedEnvValue;
        }

        // 2. User plugins directory
        var userPluginRoot = NormalizeUserPluginRoot(
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile));
        if (userPluginRoot is not null)
        {
            foreach (var name in PluginExeNames())
                yield return Path.Combine(userPluginRoot, kindWire, extBare, name);
        }

        // 3. Bundled directory (next to the main executable)
        var appDir = AppContext.BaseDirectory;
        foreach (var name in PluginExeNames())
            yield return Path.Combine(appDir, "plugins", kindWire, extBare, name);

        // 4. PATH lookup
        foreach (var pathExe in PathCandidates(kindWire, extBare))
            yield return pathExe;
    }

    /// <summary>
    /// All convention directories considered for full-machine enumeration. Used
    /// by <see cref="EnumerateAll"/> to discover everything regardless of kind.
    /// </summary>
    private static IEnumerable<string> CandidateDirectories()
    {
        var userPluginRoot = NormalizeUserPluginRoot(
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile));
        if (userPluginRoot is not null)
            yield return userPluginRoot;
        yield return Path.Combine(AppContext.BaseDirectory, "plugins");
    }

    private static string? NormalizeUserPluginRoot(string? userProfile)
    {
        if (string.IsNullOrWhiteSpace(userProfile) ||
            !TryNormalizeFullyQualifiedPath(userProfile, out var normalizedProfile))
        {
            return null;
        }

        return Path.Combine(normalizedProfile, ".officecli", "plugins");
    }

    private static IEnumerable<string> PluginExeNames()
    {
        if (OperatingSystem.IsWindows())
        {
            yield return "plugin.exe";
            yield return "plugin";
        }
        else
        {
            yield return "plugin";
        }
    }

    /// <summary>
    /// PATH lookup for binaries named `officecli-<kind>-<ext>(.exe)` or, as a
    /// fallback, `officecli-<ext>(.exe)`. The latter is convenient for plugins
    /// that only implement one kind for one extension.
    /// </summary>
    private static IEnumerable<string> PathCandidates(string kindWire, string extBare)
    {
        var safePathDirs = SafePathDirectories();

        var nameVariants = new[]
        {
            $"officecli-{kindWire}-{extBare}",
            $"officecli-{extBare}",
        };

        // The protocol gives the kind-qualified alias global priority over the
        // short alias. Preserve PATH directory order within each alias class.
        foreach (var stem in nameVariants)
        {
            foreach (var dir in safePathDirs)
            {
                if (OperatingSystem.IsWindows())
                {
                    yield return Path.Combine(dir, stem + ".exe");
                    yield return Path.Combine(dir, stem);
                }
                else
                {
                    yield return Path.Combine(dir, stem);
                }
            }
        }
    }

    /// <summary>
    /// True if <paramref name="dir"/> is world-writable (Unix other-write bit).
    /// Always false on Windows (uses ACLs, not mode bits) and on any error —
    /// failing open here only means we don't add an extra skip, the directory
    /// is still subject to normal File.Exists resolution.
    /// </summary>
    internal static bool IsWorldWritableDir(string dir)
    {
        if (OperatingSystem.IsWindows()) return false;
        try
        {
            if (!Directory.Exists(dir)) return false;
            var mode = File.GetUnixFileMode(dir);
            return (mode & UnixFileMode.OtherWrite) != 0;
        }
        catch { return false; }
    }

    private static IEnumerable<string> EnumerateExecutablesUnder(string root)
    {
        // Two-level layout: <root>/<kind>/<ext>/plugin(.exe)
        var scanBudget = new DirectoryScanBudget();
        var kindDirs = EnumerateDirectoriesOrderedBounded(root, scanBudget);

        foreach (var kindDir in kindDirs)
        {
            var extDirs = EnumerateDirectoriesOrderedBounded(kindDir, scanBudget);

            foreach (var extDir in extDirs)
            {
                foreach (var name in PluginExeNames())
                {
                    var candidate = Path.Combine(extDir, name);
                    if (File.Exists(candidate)) yield return candidate;
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Manifest invocation
    // ---------------------------------------------------------------------

    /// <summary>
    /// Supported plugin protocol major version. The registry rejects any
    /// manifest whose <c>protocol</c> differs from this value (per §13).
    /// </summary>
    public const int SupportedProtocolVersion = 1;

    /// <summary>
    /// Run <c>plugin --info</c> and parse the resulting JSON. Returns false
    /// (and swallows the exception) if the plugin times out, exits non-zero,
    /// emits malformed JSON, or declares an incompatible protocol version.
    /// Callers should treat false the same way they treat "plugin not found".
    /// </summary>
    public static bool TryReadManifest(string executablePath, out PluginManifest manifest)
        => TryReadManifest(executablePath, out manifest, out _);

    internal static bool TryReadManifest(
        string executablePath,
        out PluginManifest manifest,
        out string manifestIdentity)
        => TryReadManifest(
            executablePath,
            out manifest,
            out manifestIdentity,
            out _,
            out _,
            InfoTimeoutMs);

    internal static bool TryReadManifest(
        string executablePath,
        out PluginManifest manifest,
        out string manifestIdentity,
        out string rawManifest)
        => TryReadManifest(
            executablePath,
            out manifest,
            out manifestIdentity,
            out rawManifest,
            out _,
            InfoTimeoutMs);

    private static bool TryReadManifest(
        string executablePath,
        out PluginManifest manifest,
        out string manifestIdentity,
        out string rawManifest,
        out int manifestOutputBytes,
        int timeoutMs)
    {
        manifest = new PluginManifest();
        manifestIdentity = "";
        rawManifest = "";
        manifestOutputBytes = 0;
        try
        {
            // Process.Start() is synchronous and cannot be preempted portably.
            // Start the deadline before invoking it so any launch delay is at
            // least charged to the probe and a late return is rejected.
            var probeTimer = Stopwatch.StartNew();
            using var p = new Process
            {
                StartInfo = new ProcessStartInfo
                {
                    FileName = executablePath,
                    Arguments = "--info",
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    // CONSISTENCY(child-stream-encoding): see BlankDocCreator.
                    StandardOutputEncoding = System.Text.Encoding.UTF8,
                    StandardErrorEncoding = System.Text.Encoding.UTF8,
                    CreateNoWindow = true,
                }
            };
            if (!p.Start()) return false;
            var remainingMs = timeoutMs - (int)probeTimer.ElapsedMilliseconds;
            if (remainingMs <= 0)
            {
                try { p.Kill(entireProcessTree: true); } catch { }
                return false;
            }

            // Start the async stdout/stderr reads BEFORE WaitForExit. Synchronous
            // read-after-wait deadlocks when manifest output exceeds the pipe
            // buffer (rare for manifests, but happens when plugins emit verbose
            // diagnostics on stderr alongside --info).
            var stdoutOverflow = new TaskCompletionSource<bool>(
                TaskCreationOptions.RunContinuationsAsynchronously);
            var exitTask = p.WaitForExitAsync();
            var timeoutTask = Task.Delay(Math.Max(1, remainingMs));
            var stdoutTask = ReadBoundedAsync(
                p.StandardOutput.BaseStream,
                ManifestOutputByteLimit,
                stdoutOverflow);
            var stderrTask = ReadBoundedAsync(
                p.StandardError.BaseStream,
                ManifestDiagnosticRetainedByteLimit,
                overflowSignal: null);
            var completedTask = Task.WhenAny(exitTask, stdoutOverflow.Task, timeoutTask)
                .GetAwaiter()
                .GetResult();
            var aborted = !ReferenceEquals(completedTask, exitTask);
            if (aborted)
            {
                try { p.Kill(entireProcessTree: true); } catch { }
            }

            remainingMs = Math.Max(0, timeoutMs - (int)probeTimer.ElapsedMilliseconds);
            if (remainingMs > 0)
            {
                try { p.WaitForExit(Math.Min(1000, remainingMs)); } catch { }
            }
            remainingMs = Math.Max(0, timeoutMs - (int)probeTimer.ElapsedMilliseconds);
            if (remainingMs <= 0 ||
                !Task.WaitAll([stdoutTask, stderrTask], Math.Min(1000, remainingMs)))
            {
                ObserveFaults(exitTask, stdoutTask, stderrTask);
                return false;
            }
            if (aborted) return false;

            var stdoutResult = stdoutTask.GetAwaiter().GetResult();
            _ = stderrTask.GetAwaiter().GetResult();
            if (p.ExitCode != 0 || stdoutResult.Exceeded) return false;

            var stdout = StrictUtf8.GetString(stdoutResult.Bytes);
            if (string.IsNullOrWhiteSpace(stdout)) return false;

            var manifestJson = TrimLeadingBom(stdout);
            var parsed = JsonSerializer.Deserialize(manifestJson, PluginJsonContext.Default.PluginManifest);
            if (parsed is null) return false;

            // System.Text.Json permits an explicit JSON null to overwrite
            // non-nullable property initializers. Reject those registrations
            // before list or resolution code dereferences required fields.
            if (!HasSafeRequiredFields(parsed)) return false;

            // Protocol gate. Mismatch is fatal — we will not load a plugin that
            // implements a different major version, since the wire format may
            // differ in ways main does not understand. Surface a one-line
            // warning so users debugging "plugin not found" can see that an
            // installed plugin was rejected for version reasons — silent
            // rejection would leave them guessing.
            if (parsed.Protocol != SupportedProtocolVersion)
            {
                Console.Error.WriteLine(
                    $"[warning] plugin at {executablePath} declares protocol={parsed.Protocol} " +
                    $"but main supports protocol={SupportedProtocolVersion}; plugin will not load.");
                return false;
            }

            manifest = parsed;
            manifestIdentity = ComputeManifestIdentity(manifestJson);
            rawManifest = manifestJson;
            manifestOutputBytes = stdoutResult.Bytes.Length;
            return true;
        }
        catch
        {
            return false;
        }
    }

    // ---------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------

    private sealed record ManifestMetadata(string Identity);
    private sealed record BoundedReadResult(byte[] Bytes, bool Exceeded);

    private sealed class DirectoryScanBudget
    {
        private int _count;

        public void Consume()
        {
            _count++;
            if (_count > MaxManagedDirectoryEntriesScanned)
            {
                throw DiscoveryLimit(
                    $"more than {MaxManagedDirectoryEntriesScanned} managed plugin directory entries were scanned");
            }
        }
    }

    internal static ResolvedPlugin CreateResolvedPlugin(
        string executablePath,
        PluginManifest manifest,
        string manifestIdentity)
    {
        var plugin = new ResolvedPlugin(executablePath, manifest);
        if (!string.IsNullOrEmpty(manifestIdentity))
            _manifestIdentities.Add(plugin, new ManifestMetadata(manifestIdentity));
        return plugin;
    }

    private static IReadOnlyList<string> MaterializeDiscoveryCandidates()
    {
        IEnumerable<string> DiscoveryCandidates()
        {
            // Keep the same broad priority classes as per-operation discovery.
            // Environment registrations are explicit, followed by managed
            // install roots and convention-shaped executables on PATH.
            foreach (var executablePath in EnvironmentOverrideCandidates())
                yield return executablePath;

            foreach (var dir in CandidateDirectories())
            {
                if (!Directory.Exists(dir)) continue;
                foreach (var executablePath in EnumerateExecutablesUnder(dir))
                    yield return executablePath;
            }

            foreach (var executablePath in PathExecutableCandidates())
                yield return executablePath;
        }

        return NormalizeExistingCandidates(
            DiscoveryCandidates(),
            MaxDiscoveryCandidates);
    }

    internal static IReadOnlyList<string> NormalizeExistingCandidates(
        IEnumerable<string> candidates,
        int maxCandidates)
    {
        var normalizedCandidates = new List<string>();
        var seen = new HashSet<string>(PathComparer);
        foreach (var candidate in candidates)
        {
            if (!TryNormalizeFullyQualifiedPath(candidate, out var normalized)) continue;
            if (!File.Exists(normalized) || !seen.Add(normalized)) continue;
            normalizedCandidates.Add(normalized);
            if (normalizedCandidates.Count <= maxCandidates) continue;

            throw DiscoveryLimit(
                $"more than {maxCandidates} unique executable candidates were found");
        }
        return normalizedCandidates;
    }

    private static CliException DiscoveryLimit(string reason, string? suggestion = null) => new(
        $"Plugin discovery limit exceeded ({reason}); refusing to return a partial list.")
    {
        Code = "plugin_discovery_limit",
        Suggestion = suggestion ??
            "Remove stale plugin registrations or narrow PATH, then run `officecli plugins list` again.",
    };

    private static CliException DiscoveryTimeout(int probeBudgetMs) => new(
        $"Plugin discovery exceeded its {probeBudgetMs / 1000.0:0.###}-second probe budget; no partial list was returned.")
    {
        Code = "plugin_discovery_timeout",
        Suggestion = "Remove or repair slow plugin registrations, then run `officecli plugins list` again.",
    };

    /// <summary>
    /// Resolve a stable manifest name without silently choosing between
    /// different plugins. Identical registrations use discovery priority;
    /// conflicting full manifests require an explicit executable path.
    /// </summary>
    internal static ResolvedPlugin? ResolveByStableName(
        IEnumerable<ResolvedPlugin> plugins,
        string name)
    {
        var matches = plugins
            .Where(plugin => string.Equals(
                plugin.Manifest.Name,
                name,
                StringComparison.OrdinalIgnoreCase))
            .ToList();
        if (matches.Count == 0) return null;

        var firstIdentity = EffectiveManifestIdentity(matches[0]);
        if (matches.Skip(1).Any(plugin =>
            !string.Equals(
                EffectiveManifestIdentity(plugin),
                firstIdentity,
                StringComparison.Ordinal)))
        {
            throw new CliException(
                $"Plugin name '{name}' is ambiguous because multiple non-identical manifests are registered.")
            {
                Code = "plugin_name_ambiguous",
                Suggestion = "Run `officecli plugins list` to inspect registrations, then provide the absolute path to the intended plugin executable.",
            };
        }

        return matches[0];
    }

    /// <summary>
    /// Compare a freshly probed manifest with the immutable discovery snapshot
    /// used to select a registration. This closes the check/use gap for
    /// commands that need to re-read unknown future fields from <c>--info</c>.
    /// </summary>
    internal static bool ManifestIdentityMatches(
        ResolvedPlugin plugin,
        string manifestIdentity) =>
        string.Equals(
            EffectiveManifestIdentity(plugin),
            manifestIdentity,
            StringComparison.Ordinal);

    /// <summary>
    /// Return list-row warnings caused by registration identity conflicts.
    /// Paths stay in the list's dedicated path field rather than being copied
    /// into warnings or one-line errors.
    /// </summary>
    internal static IReadOnlyDictionary<string, IReadOnlyList<string>> RegistrationWarningsFor(
        IReadOnlyList<ResolvedPlugin> plugins)
    {
        var warnings = new Dictionary<string, IReadOnlyList<string>>(PathComparer);
        foreach (var sameName in plugins.GroupBy(
            plugin => plugin.Manifest.Name,
            StringComparer.OrdinalIgnoreCase))
        {
            var registrations = sameName.ToList();
            var distinctIdentities = registrations
                .Select(EffectiveManifestIdentity)
                .Distinct(StringComparer.Ordinal)
                .Count();
            if (distinctIdentities <= 1) continue;

            IReadOnlyList<string> conflictWarning =
            [
                $"stable name is shared by {distinctIdentities} non-identical manifests across {registrations.Count} paths; name-based `plugins info` and `plugins lint` are ambiguous",
            ];
            foreach (var registration in registrations)
                warnings[registration.ExecutablePath] = conflictWarning;
        }
        return warnings;
    }

    /// <summary>
    /// Hash canonical JSON so future fields participate in identity while
    /// insignificant object ordering, whitespace, and a UTF-8 BOM do not.
    /// Array order and scalar representations are deliberately preserved.
    /// </summary>
    internal static string ComputeManifestIdentity(string manifestJson)
    {
        using var document = JsonDocument.Parse(TrimLeadingBom(manifestJson));
        using var stream = new MemoryStream();
        using (var writer = new Utf8JsonWriter(stream))
        {
            WriteCanonicalJson(writer, document.RootElement);
        }
        return Convert.ToHexString(SHA256.HashData(stream.ToArray()));
    }

    private static void WriteCanonicalJson(Utf8JsonWriter writer, JsonElement element)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.Object:
                writer.WriteStartObject();
                foreach (var property in element.EnumerateObject().OrderBy(
                    property => property.Name,
                    StringComparer.Ordinal))
                {
                    writer.WritePropertyName(property.Name);
                    WriteCanonicalJson(writer, property.Value);
                }
                writer.WriteEndObject();
                break;
            case JsonValueKind.Array:
                writer.WriteStartArray();
                foreach (var item in element.EnumerateArray())
                    WriteCanonicalJson(writer, item);
                writer.WriteEndArray();
                break;
            case JsonValueKind.String:
                writer.WriteStringValue(element.GetString());
                break;
            case JsonValueKind.Number:
                writer.WriteRawValue(element.GetRawText());
                break;
            case JsonValueKind.True:
                writer.WriteBooleanValue(true);
                break;
            case JsonValueKind.False:
                writer.WriteBooleanValue(false);
                break;
            case JsonValueKind.Null:
                writer.WriteNullValue();
                break;
            default:
                throw new JsonException($"Unsupported manifest JSON token: {element.ValueKind}");
        }
    }

    private static IReadOnlyList<string> EnumerateDirectoriesOrderedBounded(
        string root,
        DirectoryScanBudget scanBudget)
    {
        var directories = new List<string>();
        try
        {
            foreach (var directory in Directory.EnumerateDirectories(root))
            {
                scanBudget.Consume();
                directories.Add(directory);
            }
        }
        catch (CliException)
        {
            throw;
        }
        catch
        {
            return Array.Empty<string>();
        }

        directories.Sort(PathComparer);
        return directories;
    }

    private static async Task<BoundedReadResult> ReadBoundedAsync(
        Stream stream,
        int maxRetainedBytes,
        TaskCompletionSource<bool>? overflowSignal)
    {
        // PipeStream.ReadAsync is allowed to complete synchronously. Force the
        // reader off the caller before entering a potentially unending flood so
        // the timeout/overflow monitor above always gets a chance to run.
        await Task.Yield();
        var buffer = new byte[8192];
        using var retained = new MemoryStream(
            capacity: Math.Min(maxRetainedBytes, buffer.Length));
        var exceeded = false;
        while (true)
        {
            var count = await stream.ReadAsync(buffer.AsMemory(0, buffer.Length));
            if (count == 0) break;

            var remaining = maxRetainedBytes - checked((int)retained.Length);
            var retainCount = Math.Min(count, Math.Max(0, remaining));
            if (retainCount > 0)
                retained.Write(buffer, 0, retainCount);

            if (retainCount < count && !exceeded)
            {
                exceeded = true;
                overflowSignal?.TrySetResult(true);
            }
        }
        return new BoundedReadResult(retained.ToArray(), exceeded);
    }

    private static void ObserveFaults(params Task[] tasks)
    {
        _ = Task.WhenAll(tasks).ContinueWith(
            completed => _ = completed.Exception,
            CancellationToken.None,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously,
            TaskScheduler.Default);
    }

    private static bool HasSafeRequiredFields(PluginManifest manifest) =>
        !string.IsNullOrWhiteSpace(manifest.Name) &&
        !string.IsNullOrWhiteSpace(manifest.Version) &&
        manifest.Kinds is { Count: > 0 } &&
        manifest.Kinds.All(kind => !string.IsNullOrWhiteSpace(kind)) &&
        manifest.Extensions is { Count: > 0 } &&
        manifest.Extensions.All(extension => !string.IsNullOrWhiteSpace(extension));

    /// <summary>
    /// Absolute plugin executables configured through protocol environment
    /// variables. Enumeration is deterministic and ignores unrelated host
    /// settings such as the idle-timeout override.
    /// </summary>
    private static IEnumerable<string> EnvironmentOverrideCandidates()
    {
        var registrations = Environment.GetEnvironmentVariables()
            .Cast<DictionaryEntry>()
            .Select(entry => (
                Name: entry.Key?.ToString() ?? "",
                Value: entry.Value?.ToString() ?? ""))
            .Where(entry => IsPluginEnvironmentVariable(entry.Name))
            .OrderBy(entry => entry.Name, StringComparer.OrdinalIgnoreCase)
            .ThenBy(entry => entry.Name, StringComparer.Ordinal);

        foreach (var (_, value) in registrations)
            if (TryNormalizeFullyQualifiedPath(value, out var normalized))
                yield return normalized;
    }

    /// <summary>
    /// Enumerate PATH registrations whose filenames implement the protocol's
    /// qualified or short alias shape. Qualified aliases retain global
    /// priority over short aliases; directory order is stable within a class.
    /// </summary>
    private static IEnumerable<string> PathExecutableCandidates()
    {
        var ranked = new List<(
            int AliasRank,
            int DirectoryIndex,
            string Stem,
            int ExecutableRank,
            string Path)>();
        var seen = new HashSet<string>(PathComparer);
        var scannedEntries = 0;
        var directories = SafePathDirectories();
        for (var directoryIndex = 0; directoryIndex < directories.Count; directoryIndex++)
        {
            IEnumerable<string> files;
            try
            {
                files = Directory.EnumerateFiles(directories[directoryIndex], "officecli-*");
            }
            catch
            {
                continue;
            }

            var directoryRanked = new List<(
                int AliasRank,
                int DirectoryIndex,
                string Stem,
                int ExecutableRank,
                string Path)>();
            var directorySeen = new HashSet<string>(PathComparer);
            try
            {
                foreach (var path in files)
                {
                    scannedEntries++;
                    if (scannedEntries > MaxPathEntriesScanned)
                    {
                        throw DiscoveryLimit(
                            $"more than {MaxPathEntriesScanned} officecli-* PATH entries were scanned");
                    }
                    if (!TryClassifyPathAlias(path, out var rank) ||
                        !TryNormalizeFullyQualifiedPath(path, out var normalized) ||
                        seen.Contains(normalized) ||
                        !directorySeen.Add(normalized))
                    {
                        continue;
                    }
                    if (ranked.Count + directoryRanked.Count >= MaxDiscoveryCandidates)
                    {
                        throw DiscoveryLimit(
                            $"more than {MaxDiscoveryCandidates} unique protocol-shaped PATH executables were found");
                    }

                    var fileName = Path.GetFileName(path);
                    var isWindowsExe = OperatingSystem.IsWindows() &&
                        fileName.EndsWith(".exe", StringComparison.OrdinalIgnoreCase);
                    var stem = isWindowsExe
                        ? fileName.Substring(0, fileName.Length - 4)
                        : fileName;
                    directoryRanked.Add((rank, directoryIndex, stem, isWindowsExe ? 0 : 1, normalized));
                }

                foreach (var candidate in directoryRanked)
                {
                    seen.Add(candidate.Path);
                    ranked.Add(candidate);
                }
            }
            catch (CliException)
            {
                throw;
            }
            catch
            {
                // A directory can disappear or become unreadable mid-scan.
                // Discard every candidate collected from that directory rather
                // than retaining a partial, order-dependent view of it.
            }
        }

        foreach (var candidate in ranked
            .OrderBy(candidate => candidate.AliasRank)
            .ThenBy(candidate => candidate.DirectoryIndex)
            .ThenBy(candidate => candidate.Stem, PathComparer)
            .ThenBy(candidate => candidate.ExecutableRank)
            .ThenBy(candidate => candidate.Path, PathComparer))
        {
            yield return candidate.Path;
        }
    }

    private static IReadOnlyList<string> SafePathDirectories()
    {
        var pathDirs = (Environment.GetEnvironmentVariable("PATH") ?? "")
            .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries);
        var safe = new List<string>();
        var seen = new HashSet<string>(PathComparer);
        foreach (var dir in pathDirs)
        {
            // A rooted Windows path such as `\plugins` still depends on the
            // current drive. Only fully-qualified entries are deterministic.
            if (!TryNormalizeFullyQualifiedPath(dir, out var normalized)) continue;
            if (IsWorldWritableDir(normalized)) continue;
            if (seen.Add(normalized)) safe.Add(normalized);
        }
        return safe;
    }

    private static bool IsPluginEnvironmentVariable(string name)
    {
        var comparison = OperatingSystem.IsWindows()
            ? StringComparison.OrdinalIgnoreCase
            : StringComparison.Ordinal;
        string[] prefixes =
        [
            "OFFICECLI_PLUGIN_DUMP_READER_",
            "OFFICECLI_PLUGIN_EXPORTER_",
            "OFFICECLI_PLUGIN_FORMAT_HANDLER_",
        ];
        foreach (var prefix in prefixes)
        {
            if (!name.StartsWith(prefix, comparison)) continue;
            return IsEnvironmentAliasToken(name.Substring(prefix.Length));
        }
        return false;
    }

    private static bool TryClassifyPathAlias(string path, out int rank)
    {
        rank = 0;
        var fileName = Path.GetFileName(path);
        if (OperatingSystem.IsWindows() && fileName.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
            fileName = fileName.Substring(0, fileName.Length - 4);
        else if (Path.HasExtension(fileName))
            return false;

        const string prefix = "officecli-";
        var comparison = OperatingSystem.IsWindows()
            ? StringComparison.OrdinalIgnoreCase
            : StringComparison.Ordinal;
        if (!fileName.StartsWith(prefix, comparison)) return false;
        var alias = fileName.Substring(prefix.Length);

        string[] kinds = ["dump-reader-", "exporter-", "format-handler-"];
        foreach (var kind in kinds)
        {
            if (!alias.StartsWith(kind, comparison)) continue;
            rank = 0;
            return IsPathAliasToken(alias.Substring(kind.Length));
        }

        rank = 1;
        return IsPathAliasToken(alias);
    }

    private static bool IsEnvironmentAliasToken(string value) =>
        value.Length > 0 && value.All(character => char.IsAsciiDigit(character) ||
            (OperatingSystem.IsWindows()
                ? char.IsAsciiLetter(character)
                : character is >= 'A' and <= 'Z'));

    private static bool IsPathAliasToken(string value) =>
        value.Length > 0 && value.All(character => char.IsAsciiDigit(character) ||
            (OperatingSystem.IsWindows()
                ? char.IsAsciiLetter(character)
                : character is >= 'a' and <= 'z'));

    private static string EffectiveManifestIdentity(ResolvedPlugin plugin)
    {
        if (_manifestIdentities.TryGetValue(plugin, out var metadata) &&
            !string.IsNullOrEmpty(metadata.Identity))
        {
            return metadata.Identity;
        }
        var json = JsonSerializer.Serialize(plugin.Manifest, PluginJsonContext.Default.PluginManifest);
        return ComputeManifestIdentity(json);
    }

    private static string TrimLeadingBom(string value)
    {
        var trimmed = value.TrimStart();
        return trimmed.Length > 0 && trimmed[0] == '\uFEFF'
            ? trimmed.Substring(1).TrimStart()
            : trimmed;
    }

    private static bool TryNormalizeFullyQualifiedPath(string path, out string normalized)
    {
        normalized = "";
        if (string.IsNullOrWhiteSpace(path) || !Path.IsPathFullyQualified(path)) return false;
        try
        {
            normalized = Path.GetFullPath(path);
            return true;
        }
        catch
        {
            return false;
        }
    }

    private static bool ManifestMatches(PluginManifest m, PluginKind kind, string ext)
    {
        var kindWire = kind.ToWireString();
        if (!m.Kinds.Contains(kindWire)) return false;
        if (!m.Extensions.Any(e => string.Equals(NormalizeExt(e), ext, StringComparison.OrdinalIgnoreCase)))
            return false;
        return true;
    }

    private static string NormalizeExt(string ext)
    {
        if (string.IsNullOrEmpty(ext)) return ext;
        if (!ext.StartsWith('.')) ext = "." + ext;
        return ext.ToLowerInvariant();
    }
}
