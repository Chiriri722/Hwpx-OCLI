// Copyright 2026 OfficeCLI (https://OfficeCLI.AI)
// SPDX-License-Identifier: Apache-2.0

using System.Diagnostics;
using System.Reflection;
using System.Text.Json;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Wordprocessing;
using OfficeCli.Core;
using OfficeCli.Core.Plugins;

if (args is ["--probe-infinite-worker"])
{
    Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "infinite-flood");
    Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", null);
    return PluginRegistry.TryReadManifest(TestAppHostPath(), out _) ? 1 : 0;
}

if (args is ["--info"])
{
    var mode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var testManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    if (mode is not null || testManifest is not null)
    {
        switch (mode)
        {
            case "oversize-stdout":
                Console.Out.Write(new string('x', 1024 * 1024 + 1));
                Console.Out.Flush();
                Thread.Sleep(10000);
                return 0;
            case "noisy-stderr":
                Console.Error.Write(new string('e', 64 * 1024));
                Console.Error.Flush();
                break;
            case "invalid-utf8":
                Console.OpenStandardOutput().WriteByte(0xff);
                return 0;
            case "infinite-flood":
                var stderrThread = new Thread(() =>
                {
                    var stderr = Console.OpenStandardError();
                    var block = new byte[64 * 1024];
                    while (true) stderr.Write(block);
                })
                {
                    IsBackground = true,
                };
                stderrThread.Start();
                var stdout = Console.OpenStandardOutput();
                var stdoutBlock = new byte[64 * 1024];
                while (true) stdout.Write(stdoutBlock);
            case "large-description":
                var descriptionLength = int.Parse(
                    Environment.GetEnvironmentVariable("OFFICECLI_TEST_DESCRIPTION_LENGTH")
                    ?? "1000000");
                Console.WriteLine(ManifestJsonWithDescription(descriptionLength));
                return 0;
            case "stateful-manifest":
                var stateFile = Environment.GetEnvironmentVariable("OFFICECLI_TEST_STATE_FILE")
                    ?? throw new InvalidOperationException("missing manifest state path");
                var invocation = File.Exists(stateFile) &&
                    int.TryParse(File.ReadAllText(stateFile), out var previous)
                        ? previous + 1
                        : 1;
                File.WriteAllText(stateFile, invocation.ToString());
                var statefulName = Environment.GetEnvironmentVariable("OFFICECLI_TEST_PLUGIN_NAME")
                    ?? "officecli-stateful";
                Console.WriteLine(ManifestJson(statefulName, $"{invocation}.0.0"));
                return 0;
            case "marker":
                var marker = Environment.GetEnvironmentVariable("OFFICECLI_TEST_PROBE_MARKER")
                    ?? throw new InvalidOperationException("missing probe marker path");
                File.WriteAllText(marker, "probed");
                break;
            case "route-by-name":
                var executableName = Path.GetFileNameWithoutExtension(
                    Environment.ProcessPath ?? "");
                if (executableName.Contains("invalid", StringComparison.OrdinalIgnoreCase))
                {
                    Console.WriteLine("{");
                    return 0;
                }
                if (executableName.Contains("sleep", StringComparison.OrdinalIgnoreCase))
                {
                    Thread.Sleep(10000);
                }
                var extension = Environment.GetEnvironmentVariable("OFFICECLI_TEST_EXTENSION") ?? ".hwp";
                var version = executableName.Contains("second", StringComparison.OrdinalIgnoreCase)
                    ? "2.0.0"
                    : "1.0.0";
                Console.WriteLine(ManifestJson("officecli-e2e", version, extension));
                return 0;
        }

        Console.WriteLine(testManifest ?? ManifestJson("officecli-e2e", "1.0.0"));
        return 0;
    }
}

if (args is ["--heartbeat-child"])
{
    for (var i = 0; i < 7; i++)
    {
        Thread.Sleep(400);
        Console.Error.WriteLine("{\"heartbeat\":true}");
        Console.Error.Flush();
    }
    Console.WriteLine("completed");
    return 0;
}

var tests = new (string Name, Action Run)[]
{
    ("relative environment overrides are rejected", RelativeEnvironmentOverrideIsRejected),
    ("absolute environment overrides keep first priority", AbsoluteEnvironmentOverrideKeepsFirstPriority),
    ("user plugin root rejects unavailable and relative profiles", UserPluginRootRejectsInvalidProfiles),
    ("manifest identity ignores BOM whitespace and object property order", ManifestIdentityIsCanonical),
    ("manifest identity includes unknown future fields", ManifestIdentityIncludesUnknownFields),
    ("manifest identity preserves array order", ManifestIdentityPreservesArrayOrder),
    ("equivalent registration aliases use first discovery priority", EquivalentRegistrationAliasesUseFirstPriority),
    ("conflicting stable names are rejected as ambiguous", ConflictingStableNamesAreRejectedAsAmbiguous),
    ("conflicting registrations warn on every row without leaking paths", ConflictingRegistrationsWarnWithoutPaths),
    ("equivalent registrations do not warn", EquivalentRegistrationsDoNotWarn),
    ("registry metadata preserves public ResolvedPlugin equality", RegistryMetadataPreservesResolvedPluginEquality),
    ("HWP PATH aliases use protocol priority before directory priority", HwpPathAliasesUseProtocolPriority),
    ("equal PATH aliases preserve directory priority", EqualPathAliasesPreserveDirectoryPriority),
    ("PATH discovery rejects non-fully-qualified directories", PathDiscoveryRejectsNonFullyQualifiedDirectories),
    ("plugin enumeration includes safe environment registrations", PluginEnumerationIncludesSafeEnvironmentRegistrations),
    ("plugin environment names follow platform case rules", PluginEnvironmentNamesFollowPlatformCaseRules),
    ("plugin enumeration includes only protocol-shaped PATH aliases", PluginEnumerationIncludesProtocolPathAliases),
    ("PATH alias names follow platform case rules", PathAliasNamesFollowPlatformCaseRules),
    ("Windows PATH enumeration prefers .exe over a bare twin", WindowsPathEnumerationPrefersExeTwin),
    ("installed plugin directory enumeration is deterministic", InstalledDirectoryEnumerationIsDeterministic),
    ("malformed required manifest fields are ignored", MalformedRequiredManifestFieldsAreIgnored),
    ("manifest stdout over the hard byte cap is killed and rejected", OversizeManifestOutputIsRejectedPromptly),
    ("infinite stdout and stderr cannot starve the info watchdog", InfiniteManifestFloodIsRejectedPromptly),
    ("large manifest stderr is drained without rejecting valid stdout", LargeManifestDiagnosticsAreDrained),
    ("invalid UTF-8 manifest output is rejected", InvalidUtf8ManifestIsRejected),
    ("aggregate accepted manifest output is bounded", AggregateManifestOutputIsBounded),
    ("candidate normalization deduplicates before enforcing its limit", CandidateNormalizationDeduplicatesBeforeLimit),
    ("candidate overflow fails before any plugin probe", CandidateOverflowFailsExplicitly),
    ("PATH candidate limit accepts the documented boundary", PathCandidateLimitAcceptsBoundary),
    ("PATH candidate overflow fails before launching a plugin", PathCandidateOverflowFailsBeforeProbe),
    ("global discovery timeout never returns a partial snapshot", GlobalDiscoveryTimeoutRejectsPartialSnapshot),
    ("public resolution falls back from an invalid override to PATH", PublicResolutionFallsBackToPath),
    ("plugins list and info enforce conflict policy end to end", PluginCommandsEnforceConflictPolicyEndToEnd),
    ("plugins info rejects a changed name snapshot and probes an explicit path once", PluginInfoRejectsChangedSnapshot),
    ("host watchdog accepts heartbeats throughout a slow plugin run", HostWatchdogAcceptsHeartbeats),
    ("dump-reader surfaces only bounded structured success warnings", DumpReaderStructuredWarningsAreFilteredAndBounded),
    ("field schema accepts emitted character formatting", FieldSchemaAcceptsEmittedCharacterFormatting),
    ("note reference decorations preserve prefix suffix and baseline", NoteReferenceDecorationsArePreserved),
};

var failures = 0;
foreach (var (name, run) in tests)
{
    try
    {
        run();
        Console.WriteLine($"PASS: {name}");
    }
    catch (Exception ex)
    {
        failures++;
        Console.Error.WriteLine($"FAIL: {name}\n{ex}");
    }
}

return failures == 0 ? 0 : 1;

static void NoteReferenceDecorationsArePreserved()
{
    var path = Path.Combine(Path.GetTempPath(), $"officecli-note-decoration-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(path);
        using (var handler = new OfficeCli.Handlers.WordHandler(path, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "anchor",
            });
            handler.Add("/body/p[1]", "footnote", null, new Dictionary<string, string>
            {
                ["text"] = "footnote body",
                ["referencePrefix"] = "[",
                ["referenceSuffix"] = ")",
                ["referenceSuperscript"] = "false",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"footnote decoration props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");

            handler.Add("/body/p[1]", "endnote", null, new Dictionary<string, string>
            {
                ["text"] = "endnote body",
                ["referencePrefix"] = "<",
                ["referenceSuffix"] = ">",
                ["referenceSuperscript"] = "true",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"endnote decoration props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            handler.Save();
        }

        using (var source = new OfficeCli.Handlers.WordHandler(path, editable: false))
        {
            var items = OfficeCli.Handlers.WordBatchEmitter.EmitWord(source);
            var footnote = items.Single(item => item.Command == "add" && item.Type == "footnote");
            var endnote = items.Single(item => item.Command == "add" && item.Type == "endnote");
            Assert(footnote.Props?.GetValueOrDefault("referencePrefix") == "["
                && footnote.Props?.GetValueOrDefault("referenceSuffix") == ")",
                "footnote dump dropped its reference prefix or suffix");
            Assert(endnote.Props?.GetValueOrDefault("referencePrefix") == "<"
                && endnote.Props?.GetValueOrDefault("referenceSuffix") == ">",
                "endnote dump dropped its reference prefix or suffix");
            Assert(footnote.Props?.GetValueOrDefault("text") == "footnote body",
                $"footnote marker decoration leaked into body text: {footnote.Props?.GetValueOrDefault("text")}");
            Assert(endnote.Props?.GetValueOrDefault("text") == "endnote body",
                $"endnote marker decoration leaked into body text: {endnote.Props?.GetValueOrDefault("text")}");
            Assert(footnote.Props?.ContainsKey("referenceMarkRPr") == true
                && endnote.Props?.ContainsKey("referenceMarkRPr") == true,
                "decorated note mark run was not recognized by the dump emitter");
        }

        using var document = WordprocessingDocument.Open(path, false);
        var main = document.MainDocumentPart!;
        var footnoteRefRun = main.Document!.Descendants<Run>()
            .Single(run => run.GetFirstChild<FootnoteReference>() != null);
        AssertDecoratedReferenceRun(
            footnoteRefRun, "t,footnoteReference,t", "[", ")", VerticalPositionValues.Baseline);

        var footnoteMarkRun = main.FootnotesPart!.Footnotes!
            .Descendants<Run>()
            .Single(run => run.GetFirstChild<FootnoteReferenceMark>() != null);
        AssertDecoratedReferenceRun(
            footnoteMarkRun, "t,footnoteRef,t", "[", ")", VerticalPositionValues.Baseline);

        var endnoteRefRun = main.Document!.Descendants<Run>()
            .Single(run => run.GetFirstChild<EndnoteReference>() != null);
        AssertDecoratedReferenceRun(
            endnoteRefRun, "t,endnoteReference,t", "<", ">", VerticalPositionValues.Superscript);

        var endnoteMarkRun = main.EndnotesPart!.Endnotes!
            .Descendants<Run>()
            .Single(run => run.GetFirstChild<EndnoteReferenceMark>() != null);
        AssertDecoratedReferenceRun(
            endnoteMarkRun, "t,endnoteRef,t", "<", ">", VerticalPositionValues.Superscript);
    }
    finally
    {
        if (File.Exists(path)) File.Delete(path);
    }
}

static void AssertDecoratedReferenceRun(
    Run run,
    string expectedChildren,
    string prefix,
    string suffix,
    VerticalPositionValues verticalAlignment)
{
    var children = string.Join(",", run.ChildElements
        .Where(child => child is not RunProperties)
        .Select(child => child.LocalName));
    Assert(children == expectedChildren,
        $"reference children differ: expected {expectedChildren}, got {children}");
    var text = run.Elements<Text>().Select(element => element.Text).ToArray();
    Assert(text.SequenceEqual(new[] { prefix, suffix }),
        $"reference decorations differ: {string.Join("|", text)}");
    var align = run.RunProperties?.GetFirstChild<VerticalTextAlignment>()?.Val?.Value;
    Assert(align == verticalAlignment,
        $"reference vertical alignment differs: expected {verticalAlignment}, got {align}");
}

static void RelativeEnvironmentOverrideIsRejected()
{
    const string envName = "OFFICECLI_PLUGIN_DUMP_READER_HWP";
    var original = Environment.GetEnvironmentVariable(envName);
    try
    {
        var invalidPath = OperatingSystem.IsWindows()
            ? $"{Path.DirectorySeparatorChar}root-relative{Path.DirectorySeparatorChar}plugin.exe"
            : Path.Combine("relative", "plugin");
        Environment.SetEnvironmentVariable(envName, invalidPath);
        var candidates = InvokeStringSequence("CandidatePaths", PluginKind.DumpReader, ".hwp");
        var invalid = candidates.FirstOrDefault(path => !Path.IsPathFullyQualified(path));
        Assert(invalid is null, $"discovery returned a non-absolute candidate: {invalid}");
        Assert(!candidates.Contains(invalidPath, GetPathComparer()), "relative environment override was retained");
    }
    finally
    {
        Environment.SetEnvironmentVariable(envName, original);
    }
}

static void AbsoluteEnvironmentOverrideKeepsFirstPriority()
{
    const string envName = "OFFICECLI_PLUGIN_DUMP_READER_HWP";
    var original = Environment.GetEnvironmentVariable(envName);
    var absolute = Path.GetFullPath(Path.Combine("absolute", "env-plugin"));
    try
    {
        Environment.SetEnvironmentVariable(envName, absolute);
        var candidates = InvokeStringSequence("CandidatePaths", PluginKind.DumpReader, ".hwp");
        Assert(candidates.Count > 0, "discovery returned no candidates");
        Assert(GetPathComparer().Equals(candidates[0], absolute), "absolute environment override lost first priority");
    }
    finally
    {
        Environment.SetEnvironmentVariable(envName, original);
    }
}

static void UserPluginRootRejectsInvalidProfiles()
{
    var invalidProfiles = new List<string?>
    {
        null,
        "",
        "   ",
        Path.Combine("relative", "profile"),
        $"C:profile{Path.DirectorySeparatorChar}without-root",
    };
    if (OperatingSystem.IsWindows())
        invalidProfiles.Add($"{Path.DirectorySeparatorChar}root-relative");

    foreach (var invalidProfile in invalidProfiles)
    {
        var root = InvokeNullableString("NormalizeUserPluginRoot", invalidProfile);
        Assert(root is null,
            $"invalid user profile produced a plugin root: {invalidProfile ?? "<null>"}");
    }

    var absoluteProfile = Path.GetFullPath(Path.Combine(
        Path.GetTempPath(),
        $"officecli-profile-{Guid.NewGuid():N}"));
    var expected = Path.Combine(absoluteProfile, ".officecli", "plugins");
    var normalized = InvokeNullableString("NormalizeUserPluginRoot", absoluteProfile);
    Assert(GetPathComparer().Equals(normalized, expected),
        $"absolute user profile resolved incorrectly: {normalized}");
}

static void ManifestIdentityIsCanonical()
{
    const string compact = "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwp\"]}";
    const string reordered = "\uFEFF { \"extensions\" : [\".hwp\"], \"kinds\" : [\"dump-reader\"], \"protocol\" : 1, \"version\" : \"1.0.0\", \"name\" : \"officecli-hwpx\" }";
    Assert(
        InvokeManifestIdentity(compact) == InvokeManifestIdentity(reordered),
        "semantically equivalent manifest objects produced different identities");
}

static void ManifestIdentityIncludesUnknownFields()
{
    const string first = "{\"name\":\"officecli-hwpx\",\"protocol\":1,\"future\":{\"mode\":\"a\"}}";
    const string second = "{\"name\":\"officecli-hwpx\",\"protocol\":1,\"future\":{\"mode\":\"b\"}}";
    Assert(
        InvokeManifestIdentity(first) != InvokeManifestIdentity(second),
        "unknown future fields were discarded from registration identity");
}

static void ManifestIdentityPreservesArrayOrder()
{
    const string first = "{\"name\":\"officecli-hwpx\",\"protocol\":1,\"future\":[\"a\",\"b\"]}";
    const string second = "{\"name\":\"officecli-hwpx\",\"protocol\":1,\"future\":[\"b\",\"a\"]}";
    Assert(
        InvokeManifestIdentity(first) != InvokeManifestIdentity(second),
        "array order was erased from registration identity");
}

static void EquivalentRegistrationAliasesUseFirstPriority()
{
    var identity = InvokeManifestIdentity(ManifestJson("officecli-hwpx", "1.0.0"));
    var first = Registration("first", Manifest("officecli-hwpx", "1.0.0"), identity);
    var duplicate = Registration("second", Manifest("officecli-hwpx", "1.0.0"), identity);
    var resolved = InvokeResolveByStableName([first, duplicate], "OFFICECLI-HWPX");
    Assert(ReferenceEquals(resolved, first), "equivalent aliases must use the first discovery candidate");
}

static void ConflictingStableNamesAreRejectedAsAmbiguous()
{
    var first = Registration(
        "first",
        Manifest("officecli-hwpx", "1.0.0"),
        InvokeManifestIdentity(ManifestJson("officecli-hwpx", "1.0.0")));
    var conflicting = Registration(
        "second",
        Manifest("officecli-hwpx", "2.0.0"),
        InvokeManifestIdentity(ManifestJson("officecli-hwpx", "2.0.0")));

    try
    {
        _ = InvokeResolveByStableName([first, conflicting], "officecli-hwpx");
        throw new InvalidOperationException("conflicting registrations were silently resolved");
    }
    catch (TargetInvocationException ex) when (ex.InnerException is CliException inner)
    {
        Assert(inner.Code == "plugin_name_ambiguous", $"unexpected error code: {inner.Code}");
        Assert(inner.Message.Contains("ambiguous", StringComparison.OrdinalIgnoreCase),
            $"ambiguity error is not actionable: {inner.Message}");
        Assert(inner.Suggestion?.Contains("absolute path", StringComparison.OrdinalIgnoreCase) == true,
            "ambiguity error must direct the user to explicit-path resolution");
        Assert(!inner.Message.Contains(first.ExecutablePath, StringComparison.Ordinal),
            "ambiguity error leaked a candidate path");
        Assert(!inner.Message.Contains(conflicting.ExecutablePath, StringComparison.Ordinal),
            "ambiguity error leaked a conflicting path");
    }
}

static void ConflictingRegistrationsWarnWithoutPaths()
{
    var first = Registration(
        "first",
        Manifest("officecli-hwpx", "1.0.0"),
        InvokeManifestIdentity(ManifestJson("officecli-hwpx", "1.0.0")));
    var conflicting = Registration(
        "second",
        Manifest("officecli-hwpx", "2.0.0"),
        InvokeManifestIdentity(ManifestJson("officecli-hwpx", "2.0.0")));
    var registrations = new List<ResolvedPlugin> { first, conflicting };

    foreach (var registration in registrations)
    {
        var warnings = InvokeRegistrationWarnings(registrations, registration);
        Assert(warnings.Count == 1, "each conflicting list row must carry one identity warning");
        Assert(warnings[0].Contains("non-identical", StringComparison.OrdinalIgnoreCase),
            $"conflict warning is unclear: {warnings[0]}");
        Assert(!warnings[0].Contains(first.ExecutablePath, StringComparison.Ordinal),
            "conflict warning leaked the first path");
        Assert(!warnings[0].Contains(conflicting.ExecutablePath, StringComparison.Ordinal),
            "conflict warning leaked the second path");
    }
}

static void EquivalentRegistrationsDoNotWarn()
{
    var identity = InvokeManifestIdentity(ManifestJson("officecli-hwpx", "1.0.0"));
    var first = Registration("first", Manifest("officecli-hwpx", "1.0.0"), identity);
    var duplicate = Registration("second", Manifest("officecli-hwpx", "1.0.0"), identity);
    var registrations = new List<ResolvedPlugin> { first, duplicate };
    Assert(InvokeRegistrationWarnings(registrations, first).Count == 0,
        "equivalent multi-path registrations must not be reported as conflicts");
}

static void RegistryMetadataPreservesResolvedPluginEquality()
{
    var manifest = Manifest("officecli-hwpx", "1.0.0");
    var path = Path.GetFullPath(Path.Combine("same", "plugin"));
    var plain = new ResolvedPlugin(path, manifest);
    var withRegistryMetadata = Registration(
        "same",
        manifest,
        InvokeManifestIdentity(ManifestJson("officecli-hwpx", "1.0.0")));

    Assert(plain == withRegistryMetadata,
        "registry-only identity metadata changed public record equality");
    Assert(plain.GetHashCode() == withRegistryMetadata.GetHashCode(),
        "registry-only identity metadata changed the public record hash code");
}

static void HwpPathAliasesUseProtocolPriority()
{
    var firstDirectory = Path.GetFullPath(Path.Combine("absolute", "first-bin"));
    var secondDirectory = Path.GetFullPath(Path.Combine("absolute", "second-bin"));
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Environment.SetEnvironmentVariable("PATH", string.Join(Path.PathSeparator, firstDirectory, secondDirectory));
        var candidates = InvokeStringSequence("PathCandidates", "dump-reader", "hwp");
        var firstShort = candidates.FindIndex(IsShortHwpAlias);
        var lastQualified = candidates.FindLastIndex(IsQualifiedHwpAlias);
        Assert(firstShort >= 0 && lastQualified >= 0, "both HWP PATH aliases must remain supported");
        Assert(lastQualified < firstShort,
            "all kind-qualified aliases must be searched before any short alias");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
    }
}

static void EqualPathAliasesPreserveDirectoryPriority()
{
    var firstDirectory = Path.GetFullPath(Path.Combine("absolute", "first-bin"));
    var secondDirectory = Path.GetFullPath(Path.Combine("absolute", "second-bin"));
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Environment.SetEnvironmentVariable("PATH", string.Join(Path.PathSeparator, firstDirectory, secondDirectory));
        var qualified = InvokeStringSequence("PathCandidates", "dump-reader", "hwp")
            .Where(IsQualifiedHwpAlias)
            .ToList();

        Assert(qualified.Count >= 2, "expected qualified aliases from both PATH directories");
        Assert(qualified[0].StartsWith(firstDirectory, GetPathComparison()),
            "equal aliases must preserve PATH directory order");
        Assert(qualified.FindIndex(path => path.StartsWith(secondDirectory, GetPathComparison())) > 0,
            "second PATH directory alias is missing");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
    }
}

static void PathDiscoveryRejectsNonFullyQualifiedDirectories()
{
    var absolute = Path.GetFullPath(Path.Combine("absolute", "safe-bin"));
    var invalid = OperatingSystem.IsWindows()
        ? $"{Path.DirectorySeparatorChar}root-relative-bin"
        : "relative-bin";
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Environment.SetEnvironmentVariable("PATH", string.Join(Path.PathSeparator, invalid, absolute));
        var candidates = InvokeStringSequence("PathCandidates", "dump-reader", "hwp");
        Assert(candidates.All(Path.IsPathFullyQualified), "PATH discovery emitted a non-fully-qualified candidate");
        Assert(candidates.Any(path => path.StartsWith(absolute, GetPathComparison())),
            "valid absolute PATH directory was discarded");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
    }
}

static void PluginEnumerationIncludesSafeEnvironmentRegistrations()
{
    const string validName = "OFFICECLI_PLUGIN_EXPORTER_TESTENUM";
    const string invalidName = "OFFICECLI_PLUGIN_FORMAT_HANDLER_TESTENUM";
    var originalValid = Environment.GetEnvironmentVariable(validName);
    var originalInvalid = Environment.GetEnvironmentVariable(invalidName);
    var absolute = Path.GetFullPath(Path.Combine("absolute", "enumerated-plugin"));
    var relative = Path.Combine("relative", "enumerated-plugin");
    try
    {
        Environment.SetEnvironmentVariable(validName, absolute);
        Environment.SetEnvironmentVariable(invalidName, relative);
        var candidates = InvokeStringSequence("EnvironmentOverrideCandidates");
        Assert(candidates.Contains(absolute, GetPathComparer()),
            "absolute environment registration was absent from full enumeration");
        Assert(!candidates.Contains(relative, GetPathComparer()),
            "relative environment registration entered full enumeration");
    }
    finally
    {
        Environment.SetEnvironmentVariable(validName, originalValid);
        Environment.SetEnvironmentVariable(invalidName, originalInvalid);
    }
}

static void PluginEnvironmentNamesFollowPlatformCaseRules()
{
    Assert(InvokeBool("IsPluginEnvironmentVariable", "OFFICECLI_PLUGIN_DUMP_READER_HWP"),
        "canonical uppercase plugin registration name was rejected");
    Assert(
        InvokeBool("IsPluginEnvironmentVariable", "officecli_plugin_dump_reader_hwp") == OperatingSystem.IsWindows(),
        "environment registration name case handling does not match the platform");
    Assert(
        InvokeBool("IsPluginEnvironmentVariable", "OFFICECLI_PLUGIN_DUMP_READER_hwp") == OperatingSystem.IsWindows(),
        "environment extension token case handling does not match actual lookup");
    Assert(!InvokeBool("IsPluginEnvironmentVariable", "OFFICECLI_PLUGIN_IDLE_TIMEOUT_SECONDS"),
        "unrelated plugin host setting was treated as an executable registration");
}

static void PluginEnumerationIncludesProtocolPathAliases()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-path-enumeration-{Guid.NewGuid():N}");
    var first = Path.Combine(root, "first");
    var second = Path.Combine(root, "second");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var shortAlias = Path.Combine(first, "officecli-hwp" + suffix);
    var qualifiedAlias = Path.Combine(second, "officecli-dump-reader-hwp" + suffix);
    var unrelated = Path.Combine(first, "officecli-hwp.cmd");
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Directory.CreateDirectory(first);
        Directory.CreateDirectory(second);
        File.WriteAllBytes(shortAlias, []);
        File.WriteAllBytes(qualifiedAlias, []);
        File.WriteAllBytes(unrelated, []);
        Environment.SetEnvironmentVariable("PATH", string.Join(Path.PathSeparator, first, second));

        var candidates = InvokeStringSequence("PathExecutableCandidates");
        Assert(candidates.Contains(shortAlias, GetPathComparer()), "short protocol PATH alias was not enumerated");
        Assert(candidates.Contains(qualifiedAlias, GetPathComparer()),
            "kind-qualified protocol PATH alias was not enumerated");
        Assert(!candidates.Contains(unrelated, GetPathComparer()),
            "non-executable suffix was treated as a protocol PATH alias");
        Assert(candidates.FindIndex(path => GetPathComparer().Equals(path, qualifiedAlias)) <
               candidates.FindIndex(path => GetPathComparer().Equals(path, shortAlias)),
            "kind-qualified PATH aliases lost global enumeration priority");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void PathAliasNamesFollowPlatformCaseRules()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-path-case-{Guid.NewGuid():N}");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    string[] mixedCaseAliases =
    [
        Path.Combine(root, "officecli-DUMP-READER-HWP" + suffix),
        Path.Combine(root, "officecli-dump-reader-HWP" + suffix),
        Path.Combine(root, "officecli-HWP" + suffix),
    ];
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Directory.CreateDirectory(root);
        foreach (var mixedCaseAlias in mixedCaseAliases)
            File.WriteAllBytes(mixedCaseAlias, []);
        Environment.SetEnvironmentVariable("PATH", root);

        var candidates = InvokeStringSequence("PathExecutableCandidates");
        foreach (var mixedCaseAlias in mixedCaseAliases)
        {
            Assert(
                candidates.Contains(mixedCaseAlias, GetPathComparer()) == OperatingSystem.IsWindows(),
                $"PATH alias filename case handling does not match actual platform resolution: {mixedCaseAlias}");
        }
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void WindowsPathEnumerationPrefersExeTwin()
{
    if (!OperatingSystem.IsWindows()) return;

    var root = Path.Combine(Path.GetTempPath(), $"officecli-path-exe-tie-{Guid.NewGuid():N}");
    var bare = Path.Combine(root, "officecli-dump-reader-tie");
    var executable = bare + ".exe";
    var original = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Directory.CreateDirectory(root);
        File.WriteAllBytes(bare, []);
        File.WriteAllBytes(executable, []);
        Environment.SetEnvironmentVariable("PATH", root);

        var candidates = InvokeStringSequence("PathExecutableCandidates");
        var executableIndex = candidates.FindIndex(path => GetPathComparer().Equals(path, executable));
        var bareIndex = candidates.FindIndex(path => GetPathComparer().Equals(path, bare));
        Assert(executableIndex >= 0 && bareIndex >= 0, "expected both Windows alias twins");
        Assert(executableIndex < bareIndex,
            "full PATH enumeration disagrees with runtime lookup's .exe-first order");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", original);
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void MalformedRequiredManifestFieldsAreIgnored()
{
    const string envName = "OFFICECLI_TEST_MANIFEST_JSON";
    var original = Environment.GetEnvironmentVariable(envName);
    var appHost = TestAppHostPath();
    string[] manifests =
    [
        "{\"name\":null,\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":null,\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":null,\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":null}",
        "{\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[null],\"extensions\":[\".hwp\"]}",
        "{\"name\":\"officecli-hwpx\",\"version\":\"1.0.0\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":[\"\"]}",
    ];

    try
    {
        foreach (var manifest in manifests)
        {
            Environment.SetEnvironmentVariable(envName, manifest);
            Assert(!PluginRegistry.TryReadManifest(appHost, out _),
                $"unsafe manifest was accepted: {manifest}");
        }
    }
    finally
    {
        Environment.SetEnvironmentVariable(envName, original);
    }
}

static void OversizeManifestOutputIsRejectedPromptly()
{
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    try
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "oversize-stdout");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", null);
        var timer = Stopwatch.StartNew();
        Assert(!PluginRegistry.TryReadManifest(TestAppHostPath(), out _),
            "oversized manifest stdout was accepted");
        timer.Stop();
        Assert(timer.Elapsed < TimeSpan.FromSeconds(4),
            $"stdout cap did not terminate the child promptly: {timer.Elapsed}");
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
    }
}

static void InfiniteManifestFloodIsRejectedPromptly()
{
    var timer = Stopwatch.StartNew();
    var worker = RunProcess(
        TestAppHostPath(),
        ["--probe-infinite-worker"],
        timeoutMs: 8000);
    timer.Stop();
    Assert(worker.ExitCode == 0,
        $"isolated infinite-flood probe failed: {worker.Stderr}\n{worker.Stdout}");
    Assert(timer.Elapsed < TimeSpan.FromSeconds(4),
        $"stream flood starved the info watchdog: {timer.Elapsed}");
}

static void LargeManifestDiagnosticsAreDrained()
{
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    try
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "noisy-stderr");
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            ManifestJson("officecli-noisy", "1.0.0"));
        Assert(PluginRegistry.TryReadManifest(TestAppHostPath(), out var manifest),
            "valid manifest was rejected because diagnostic stderr exceeded the retained prefix");
        Assert(manifest.Name == "officecli-noisy", "wrong manifest parsed after draining stderr");
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
    }
}

static void InvalidUtf8ManifestIsRejected()
{
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    try
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "invalid-utf8");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", null);
        Assert(!PluginRegistry.TryReadManifest(TestAppHostPath(), out _),
            "invalid UTF-8 was accepted as manifest text");
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
    }
}

static void AggregateManifestOutputIsBounded()
{
    var appHost = TestAppHostPath();
    var outputDirectory = Path.GetDirectoryName(appHost)
        ?? throw new InvalidOperationException("missing apphost directory");
    var token = Guid.NewGuid().ToString("N");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var candidates = Enumerable.Range(0, 17)
        .Select(index => Path.Combine(
            outputDirectory,
            $"officecli-large-{token}-{index:D2}{suffix}"))
        .ToList();
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalLength = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DESCRIPTION_LENGTH");
    try
    {
        foreach (var candidate in candidates) CopyAppHostAs(appHost, candidate);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "large-description");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DESCRIPTION_LENGTH", "1000000");

        try
        {
            _ = InvokeProbeCandidates(candidates, probeBudgetMs: 30000);
            throw new InvalidOperationException("aggregate manifest budget returned a partial list");
        }
        catch (TargetInvocationException ex) when (ex.InnerException is CliException inner)
        {
            Assert(inner.Code == "plugin_discovery_limit",
                $"unexpected aggregate manifest limit error code: {inner.Code}");
            Assert(inner.Suggestion?.Contains("manifest", StringComparison.OrdinalIgnoreCase) == true,
                $"aggregate manifest limit gave unrelated recovery guidance: {inner.Suggestion}");
        }
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DESCRIPTION_LENGTH", originalLength);
        foreach (var candidate in candidates) File.Delete(candidate);
    }
}

static void CandidateNormalizationDeduplicatesBeforeLimit()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-candidate-dedup-{Guid.NewGuid():N}");
    var candidate = Path.Combine(root, "plugin");
    try
    {
        Directory.CreateDirectory(root);
        File.WriteAllBytes(candidate, []);
        var duplicateSpelling = Path.Combine(root, ".", "plugin");
        var normalized = InvokeNormalizeExistingCandidates(
            [candidate, duplicateSpelling],
            maxCandidates: 1);
        Assert(normalized.Count == 1, "normalized duplicate consumed the candidate limit twice");
    }
    finally
    {
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void CandidateOverflowFailsExplicitly()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-candidate-limit-{Guid.NewGuid():N}");
    try
    {
        Directory.CreateDirectory(root);
        var candidates = new List<string>();
        for (var i = 0; i < 257; i++)
        {
            var candidate = Path.Combine(root, $"plugin-{i:D3}");
            File.WriteAllBytes(candidate, []);
            candidates.Add(candidate);
        }

        try
        {
            _ = InvokeNormalizeExistingCandidates(candidates, maxCandidates: 256);
            throw new InvalidOperationException("candidate overflow returned a partial list");
        }
        catch (TargetInvocationException ex) when (ex.InnerException is CliException inner)
        {
            Assert(inner.Code == "plugin_discovery_limit", $"unexpected limit error code: {inner.Code}");
        }
    }
    finally
    {
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void PathCandidateOverflowFailsBeforeProbe()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-path-limit-{Guid.NewGuid():N}");
    var marker = Path.Combine(root, "probe-marker");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var originalPath = Environment.GetEnvironmentVariable("PATH");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalMarker = Environment.GetEnvironmentVariable("OFFICECLI_TEST_PROBE_MARKER");
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    try
    {
        Directory.CreateDirectory(root);
        for (var i = 0; i < 257; i++)
        {
            var candidate = Path.Combine(root, $"officecli-dump-reader-p{i:D3}{suffix}");
            if (i == 0) CopyAppHostAs(TestAppHostPath(), candidate);
            else File.WriteAllBytes(candidate, []);
        }
        Environment.SetEnvironmentVariable("PATH", root);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "marker");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_PROBE_MARKER", marker);
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            ManifestJson("officecli-marker", "1.0.0"));

        try
        {
            _ = PluginRegistry.EnumerateAll();
            throw new InvalidOperationException("PATH candidate overflow returned a partial list");
        }
        catch (CliException ex)
        {
            Assert(ex.Code == "plugin_discovery_limit", $"unexpected PATH limit error code: {ex.Code}");
        }
        Assert(!File.Exists(marker), "plugin probing began before candidate collection completed");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", originalPath);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_PROBE_MARKER", originalMarker);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void PathCandidateLimitAcceptsBoundary()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-path-boundary-{Guid.NewGuid():N}");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var originalPath = Environment.GetEnvironmentVariable("PATH");
    try
    {
        Directory.CreateDirectory(root);
        for (var i = 0; i < 256; i++)
        {
            File.WriteAllBytes(
                Path.Combine(root, $"officecli-dump-reader-p{i:D3}{suffix}"),
                []);
        }
        Environment.SetEnvironmentVariable("PATH", root);

        var candidates = InvokeStringSequence("PathExecutableCandidates");
        Assert(candidates.Count == 256,
            $"documented 256-candidate boundary returned {candidates.Count} entries");
    }
    finally
    {
        Environment.SetEnvironmentVariable("PATH", originalPath);
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void GlobalDiscoveryTimeoutRejectsPartialSnapshot()
{
    var appHost = TestAppHostPath();
    var outputDirectory = Path.GetDirectoryName(appHost)
        ?? throw new InvalidOperationException("missing apphost directory");
    var token = Guid.NewGuid().ToString("N");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var valid = Path.Combine(outputDirectory, $"officecli-valid-{token}{suffix}");
    var sleeping = Path.Combine(outputDirectory, $"officecli-sleep-{token}{suffix}");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalExtension = Environment.GetEnvironmentVariable("OFFICECLI_TEST_EXTENSION");
    try
    {
        CopyAppHostAs(appHost, valid);
        CopyAppHostAs(appHost, sleeping);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "route-by-name");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", ".timeout");

        var timer = Stopwatch.StartNew();
        try
        {
            _ = InvokeProbeCandidates([valid, sleeping], probeBudgetMs: 500);
            throw new InvalidOperationException("global timeout returned a partial registration snapshot");
        }
        catch (TargetInvocationException ex) when (ex.InnerException is CliException inner)
        {
            timer.Stop();
            Assert(inner.Code == "plugin_discovery_timeout", $"unexpected timeout error code: {inner.Code}");
            Assert(timer.Elapsed < TimeSpan.FromSeconds(1.5),
                $"global probe deadline was not enforced promptly: {timer.Elapsed}");
        }
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", originalExtension);
        File.Delete(valid);
        File.Delete(sleeping);
    }
}

static void PublicResolutionFallsBackToPath()
{
    var appHost = TestAppHostPath();
    var outputDirectory = Path.GetDirectoryName(appHost)
        ?? throw new InvalidOperationException("missing apphost directory");
    var token = Guid.NewGuid().ToString("N");
    var extension = ".e2e" + token;
    var bareExtension = extension.TrimStart('.');
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var invalid = Path.Combine(outputDirectory, $"officecli-invalid-{token}{suffix}");
    var valid = Path.Combine(
        outputDirectory,
        $"officecli-dump-reader-{bareExtension}{suffix}");
    var registrationName = $"OFFICECLI_PLUGIN_DUMP_READER_{bareExtension.ToUpperInvariant()}";
    var originalRegistration = Environment.GetEnvironmentVariable(registrationName);
    var originalPath = Environment.GetEnvironmentVariable("PATH");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalExtension = Environment.GetEnvironmentVariable("OFFICECLI_TEST_EXTENSION");
    try
    {
        CopyAppHostAs(appHost, invalid);
        CopyAppHostAs(appHost, valid);
        Environment.SetEnvironmentVariable(registrationName, invalid);
        Environment.SetEnvironmentVariable("PATH", outputDirectory);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "route-by-name");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", extension);
        PluginRegistry.InvalidateCache();

        var resolved = PluginRegistry.FindFor(PluginKind.DumpReader, extension);
        Assert(resolved is not null, "public resolution did not fall back after the invalid override");
        Assert(GetPathComparer().Equals(resolved!.ExecutablePath, valid),
            $"public resolution selected the wrong fallback: {resolved.ExecutablePath}");
        Assert(resolved.Manifest.Extensions.Contains(extension),
            "resolved fallback manifest did not match the requested extension");
    }
    finally
    {
        PluginRegistry.InvalidateCache();
        Environment.SetEnvironmentVariable(registrationName, originalRegistration);
        Environment.SetEnvironmentVariable("PATH", originalPath);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", originalExtension);
        File.Delete(invalid);
        File.Delete(valid);
    }
}

static void PluginCommandsEnforceConflictPolicyEndToEnd()
{
    var appHost = TestAppHostPath();
    var outputDirectory = Path.GetDirectoryName(appHost)
        ?? throw new InvalidOperationException("missing apphost directory");
    var officeCliDll = Path.Combine(outputDirectory, "officecli.dll");
    Assert(File.Exists(officeCliDll), $"officecli test dependency does not exist: {officeCliDll}");
    var dotnetHost = FindExecutableOnPath(OperatingSystem.IsWindows() ? "dotnet.exe" : "dotnet")
        ?? throw new InvalidOperationException("dotnet host is unavailable for CLI contract test");

    var token = Guid.NewGuid().ToString("N");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var first = Path.Combine(outputDirectory, $"officecli-first-{token}{suffix}");
    var second = Path.Combine(outputDirectory, $"officecli-second-{token}{suffix}");
    var firstRegistration = $"OFFICECLI_PLUGIN_EXPORTER_E2E{token.ToUpperInvariant()}A";
    var secondRegistration = $"OFFICECLI_PLUGIN_EXPORTER_E2E{token.ToUpperInvariant()}B";
    var originalFirst = Environment.GetEnvironmentVariable(firstRegistration);
    var originalSecond = Environment.GetEnvironmentVariable(secondRegistration);
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalExtension = Environment.GetEnvironmentVariable("OFFICECLI_TEST_EXTENSION");
    try
    {
        CopyAppHostAs(appHost, first);
        CopyAppHostAs(appHost, second);
        Environment.SetEnvironmentVariable(firstRegistration, first);
        Environment.SetEnvironmentVariable(secondRegistration, second);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "route-by-name");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", ".e2e");

        var listed = RunProcess(dotnetHost, [officeCliDll, "plugins", "list", "--json"]);
        Assert(listed.ExitCode == 0, $"plugins list failed: {listed.Stderr}\n{listed.Stdout}");
        using (var document = JsonDocument.Parse(listed.Stdout))
        {
            var matchingRows = document.RootElement.GetProperty("data")
                .EnumerateArray()
                .Where(row => row.GetProperty("name").GetString() == "officecli-e2e")
                .ToList();
            Assert(matchingRows.Count == 2, $"expected two conflicting rows, got {matchingRows.Count}");
            Assert(matchingRows.All(row =>
                    row.TryGetProperty("warnings", out var warnings) &&
                    warnings.EnumerateArray().Any(warning =>
                        warning.GetString()?.Contains("non-identical", StringComparison.OrdinalIgnoreCase) == true)),
                "plugins list did not annotate every conflicting row");
        }

        var ambiguous = RunProcess(
            dotnetHost,
            [officeCliDll, "plugins", "info", "officecli-e2e", "--json"]);
        Assert(ambiguous.ExitCode != 0, "name-based plugins info silently selected a conflict");
        Assert((ambiguous.Stdout + ambiguous.Stderr).Contains(
                "plugin_name_ambiguous",
                StringComparison.Ordinal),
            $"ambiguity error code was not surfaced: {ambiguous.Stdout}\n{ambiguous.Stderr}");

        var explicitPath = RunProcess(
            dotnetHost,
            [officeCliDll, "plugins", "info", first, "--json"]);
        Assert(explicitPath.ExitCode == 0,
            $"explicit-path plugins info failed: {explicitPath.Stderr}\n{explicitPath.Stdout}");
        Assert(explicitPath.Stdout.Contains("future", StringComparison.Ordinal),
            "plugins info discarded an unknown future manifest field");
    }
    finally
    {
        Environment.SetEnvironmentVariable(firstRegistration, originalFirst);
        Environment.SetEnvironmentVariable(secondRegistration, originalSecond);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_EXTENSION", originalExtension);
        File.Delete(first);
        File.Delete(second);
    }
}

static void PluginInfoRejectsChangedSnapshot()
{
    var appHost = TestAppHostPath();
    var outputDirectory = Path.GetDirectoryName(appHost)
        ?? throw new InvalidOperationException("missing apphost directory");
    var officeCliDll = Path.Combine(outputDirectory, "officecli.dll");
    Assert(File.Exists(officeCliDll), $"officecli test dependency does not exist: {officeCliDll}");
    var dotnetHost = FindExecutableOnPath(OperatingSystem.IsWindows() ? "dotnet.exe" : "dotnet")
        ?? throw new InvalidOperationException("dotnet host is unavailable for CLI contract test");

    var token = Guid.NewGuid().ToString("N");
    var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
    var plugin = Path.Combine(outputDirectory, $"officecli-stateful-{token}{suffix}");
    var stateFile = Path.Combine(outputDirectory, $"manifest-state-{token}.txt");
    var pluginName = $"officecli-stateful-{token}";
    var registrationName = $"OFFICECLI_PLUGIN_EXPORTER_STATEFUL{token.ToUpperInvariant()}";
    var originalRegistration = Environment.GetEnvironmentVariable(registrationName);
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE");
    var originalStateFile = Environment.GetEnvironmentVariable("OFFICECLI_TEST_STATE_FILE");
    var originalPluginName = Environment.GetEnvironmentVariable("OFFICECLI_TEST_PLUGIN_NAME");
    try
    {
        CopyAppHostAs(appHost, plugin);
        Environment.SetEnvironmentVariable(registrationName, plugin);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", "stateful-manifest");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_STATE_FILE", stateFile);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_PLUGIN_NAME", pluginName);

        var changed = RunProcess(
            dotnetHost,
            [officeCliDll, "plugins", "info", pluginName, "--json"]);
        Assert(changed.ExitCode != 0,
            "name-based plugins info accepted a manifest that changed after discovery");
        Assert((changed.Stdout + changed.Stderr).Contains(
                "plugin_manifest_changed",
                StringComparison.Ordinal),
            $"manifest snapshot error code was not surfaced: {changed.Stdout}\n{changed.Stderr}");
        Assert(File.ReadAllText(stateFile) == "2",
            "name-based plugins info did not exercise the guarded second probe");

        File.Delete(stateFile);
        var explicitPath = RunProcess(
            dotnetHost,
            [officeCliDll, "plugins", "info", plugin, "--json"]);
        Assert(explicitPath.ExitCode == 0,
            $"explicit-path plugins info failed: {explicitPath.Stderr}\n{explicitPath.Stdout}");
        Assert(File.ReadAllText(stateFile) == "1",
            "explicit-path plugins info probed its manifest more than once");
    }
    finally
    {
        Environment.SetEnvironmentVariable(registrationName, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_INFO_MODE", originalMode);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_STATE_FILE", originalStateFile);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_PLUGIN_NAME", originalPluginName);
        File.Delete(plugin);
        File.Delete(stateFile);
    }
}

static void InstalledDirectoryEnumerationIsDeterministic()
{
    var root = Path.Combine(Path.GetTempPath(), $"officecli-registry-{Guid.NewGuid():N}");
    try
    {
        var later = Path.Combine(root, "z-kind", "z-ext");
        var earlier = Path.Combine(root, "a-kind", "a-ext");
        Directory.CreateDirectory(later);
        Directory.CreateDirectory(earlier);
        File.WriteAllBytes(Path.Combine(later, OperatingSystem.IsWindows() ? "plugin.exe" : "plugin"), []);
        File.WriteAllBytes(Path.Combine(earlier, OperatingSystem.IsWindows() ? "plugin.exe" : "plugin"), []);

        var candidates = InvokeStringSequence("EnumerateExecutablesUnder", root);
        Assert(candidates.Count == 2, $"expected two candidates, got {candidates.Count}");
        Assert(candidates[0].Contains($"{Path.DirectorySeparatorChar}a-kind{Path.DirectorySeparatorChar}", GetPathComparison()),
            "kind/extension directories were not sorted deterministically");
    }
    finally
    {
        if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
    }
}

static void HostWatchdogAcceptsHeartbeats()
{
    var appHost = TestAppHostPath();

    var stdout = new List<string>();
    var timer = Stopwatch.StartNew();
    var result = PluginProcess.Run(new PluginProcess.RunOptions
    {
        ExecutablePath = appHost,
        Arguments = ["--heartbeat-child"],
        IdleTimeoutSeconds = 1,
        OnStdoutLine = stdout.Add,
    });
    timer.Stop();

    Assert(timer.Elapsed >= TimeSpan.FromSeconds(2.5),
        $"slow child exited before exercising repeated watchdog resets: {timer.Elapsed}");
    Assert(!result.IdleTimedOut, "host killed a live plugin despite valid stderr heartbeats");
    Assert(result.ExitCode == 0, $"heartbeat child exited {result.ExitCode}: {result.Stderr}");
    Assert(stdout.SequenceEqual(["completed"]), "host did not drain the slow plugin's final stdout");
    Assert(string.IsNullOrEmpty(result.Stderr), "heartbeat plumbing leaked into diagnostic stderr");
}

static void DumpReaderStructuredWarningsAreFilteredAndBounded()
{
    const string expected = "{\"severity\":\"warning\",\"code\":\"HWPX_DORMANT_NOTE_LAYOUT_NOT_MATERIALIZED\",\"sections\":[{\"section\":1}]}";
    var stderr = string.Join('\n',
        "dumped 4 batch items from sample.hwpx",
        "{\"heartbeat\":true}",
        "not json",
        "{\"severity\":\"error\",\"code\":\"NOT_A_WARNING\"}",
        "{\"severity\":\"warning\",\"code\":\"\"}",
        expected,
        "{\"severity\":\"warning\",\"code\":\"TOO_LARGE\",\"detail\":\"" + new string('x', 12 * 1024) + "\"}");

    var method = typeof(DumpReaderInvoker).GetMethod(
        "ExtractStructuredWarnings",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(DumpReaderInvoker).FullName, "ExtractStructuredWarnings");
    var warnings = method.Invoke(null, [stderr]) as IReadOnlyList<string>
        ?? throw new InvalidOperationException("ExtractStructuredWarnings returned null");

    Assert(warnings.Count == 1, $"expected one accepted warning, got {warnings.Count}");
    Assert(warnings[0] == expected, "structured warning JSON was not surfaced unchanged");
}

static void FieldSchemaAcceptsEmittedCharacterFormatting()
{
    var schemaType = typeof(DumpReaderInvoker).Assembly.GetType("OfficeCli.Help.SchemaHelpLoader")
        ?? throw new TypeLoadException("OfficeCli.Help.SchemaHelpLoader");
    var method = schemaType.GetMethod(
        "ValidateProperties",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(schemaType.FullName, "ValidateProperties");
    var props = new Dictionary<string, string>
    {
        ["fieldType"] = "page",
        ["text"] = "1",
        ["font"] = "Batang",
        ["size"] = "12pt",
        ["bold"] = "true",
        ["italic"] = "true",
        ["underline"] = "single",
        ["strike"] = "true",
        ["color"] = "#112233",
        ["highlight"] = "yellow",
        ["superscript"] = "true",
    };
    var unknown = method.Invoke(null, ["docx", "field", "add", props]) as IReadOnlyList<string>
        ?? throw new InvalidOperationException("ValidateProperties returned null");

    Assert(unknown.Count == 0,
        $"field schema rejected emitted character formatting: {string.Join(", ", unknown)}");
}

static string TestAppHostPath()
{
    var entryAssembly = Assembly.GetExecutingAssembly().Location;
    var appHost = Path.Combine(
        Path.GetDirectoryName(entryAssembly) ?? throw new InvalidOperationException("missing test output directory"),
        Path.GetFileNameWithoutExtension(entryAssembly) + (OperatingSystem.IsWindows() ? ".exe" : ""));
    Assert(File.Exists(appHost), $"test apphost does not exist: {appHost}");
    return appHost;
}

static bool IsQualifiedHwpAlias(string path) =>
    Path.GetFileNameWithoutExtension(path).Equals("officecli-dump-reader-hwp", StringComparison.OrdinalIgnoreCase);

static bool IsShortHwpAlias(string path) =>
    Path.GetFileNameWithoutExtension(path).Equals("officecli-hwp", StringComparison.OrdinalIgnoreCase);

static ResolvedPlugin Registration(string directory, PluginManifest manifest, string identity)
{
    var method = typeof(PluginRegistry).GetMethod(
        "CreateResolvedPlugin",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "CreateResolvedPlugin");
    return (ResolvedPlugin)(method.Invoke(
        null,
        [Path.GetFullPath(Path.Combine(directory, "plugin")), manifest, identity])
        ?? throw new InvalidOperationException("CreateResolvedPlugin returned null"));
}

static ResolvedPlugin? InvokeResolveByStableName(IEnumerable<ResolvedPlugin> plugins, string name)
{
    var method = typeof(PluginRegistry).GetMethod(
        "ResolveByStableName",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "ResolveByStableName");
    return method.Invoke(null, [plugins, name]) as ResolvedPlugin;
}

static List<string> InvokeRegistrationWarnings(
    IReadOnlyList<ResolvedPlugin> plugins,
    ResolvedPlugin registration)
{
    var method = typeof(PluginRegistry).GetMethod(
        "RegistrationWarningsFor",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "RegistrationWarningsFor");
    var warningMap = method.Invoke(null, [plugins])
        ?? throw new InvalidOperationException("RegistrationWarningsFor returned null");
    var tryGetValue = warningMap.GetType().GetMethod("TryGetValue")
        ?? throw new MissingMethodException(warningMap.GetType().FullName, "TryGetValue");
    object?[] args = [registration.ExecutablePath, null];
    var found = tryGetValue.Invoke(warningMap, args) as bool? ?? false;
    return found && args[1] is IEnumerable<string> warnings
        ? warnings.ToList()
        : [];
}

static List<string> InvokeNormalizeExistingCandidates(
    IEnumerable<string> candidates,
    int maxCandidates)
{
    var method = typeof(PluginRegistry).GetMethod(
        "NormalizeExistingCandidates",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "NormalizeExistingCandidates");
    var result = method.Invoke(null, [candidates, maxCandidates]) as IEnumerable<string>
        ?? throw new InvalidOperationException("NormalizeExistingCandidates returned no string sequence");
    return result.ToList();
}

static IReadOnlyList<ResolvedPlugin> InvokeProbeCandidates(
    IReadOnlyList<string> candidates,
    int probeBudgetMs)
{
    var method = typeof(PluginRegistry).GetMethod(
        "ProbeCandidates",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "ProbeCandidates");
    return method.Invoke(null, [candidates, probeBudgetMs]) as IReadOnlyList<ResolvedPlugin>
        ?? throw new InvalidOperationException("ProbeCandidates returned no registration list");
}

static string InvokeManifestIdentity(string json)
{
    var method = typeof(PluginRegistry).GetMethod(
        "ComputeManifestIdentity",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, "ComputeManifestIdentity");
    return method.Invoke(null, [json]) as string
        ?? throw new InvalidOperationException("ComputeManifestIdentity returned no identity");
}

static List<string> InvokeStringSequence(string name, params object[] args)
{
    var method = typeof(PluginRegistry).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, name);
    var result = method.Invoke(null, args) as IEnumerable<string>
        ?? throw new InvalidOperationException($"{name} returned no string sequence");
    return result.ToList();
}

static string? InvokeNullableString(string name, params object?[] args)
{
    var method = typeof(PluginRegistry).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, name);
    return method.Invoke(null, args) as string;
}

static bool InvokeBool(string name, params object[] args)
{
    var method = typeof(PluginRegistry).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(typeof(PluginRegistry).FullName, name);
    return method.Invoke(null, args) as bool?
        ?? throw new InvalidOperationException($"{name} returned no Boolean value");
}

static PluginManifest Manifest(string name, string version) => new()
{
    Name = name,
    Version = version,
    Protocol = 1,
    Kinds = ["dump-reader"],
    Extensions = [".hwp", ".hwpx"],
    Target = "docx",
    Runtime = "rust",
    IdleTimeoutSeconds = new PluginIdleTimeout { Default = 30 },
};

static string ManifestJson(string name, string version, string? extension = null) =>
    "{\"name\":\"" + name + "\",\"version\":\"" + version +
    "\",\"protocol\":1,\"kinds\":[\"dump-reader\"],\"extensions\":" +
    (extension is null ? "[\".hwp\",\".hwpx\"]" : "[\"" + extension + "\"]") + "," +
    "\"target\":\"docx\",\"runtime\":\"rust\",\"idle_timeout_seconds\":{\"default\":30}," +
    "\"future\":{\"source\":\"contract-test\"}}";

static string ManifestJsonWithDescription(int descriptionLength)
{
    var manifest = ManifestJson("officecli-large", "1.0.0");
    return manifest[..^1] + ",\"description\":\"" +
        new string('d', descriptionLength) + "\"}";
}

static void CopyAppHostAs(string source, string destination)
{
    File.Copy(source, destination, overwrite: true);
    if (!OperatingSystem.IsWindows())
        File.SetUnixFileMode(destination, File.GetUnixFileMode(source));
}

static string? FindExecutableOnPath(string fileName)
{
    foreach (var directory in (Environment.GetEnvironmentVariable("PATH") ?? "")
        .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
    {
        try
        {
            var candidate = Path.Combine(directory, fileName);
            if (File.Exists(candidate)) return Path.GetFullPath(candidate);
        }
        catch
        {
            // Ignore malformed PATH entries in this test-only lookup.
        }
    }
    return null;
}

static (int ExitCode, string Stdout, string Stderr) RunProcess(
    string executablePath,
    IReadOnlyList<string> arguments,
    int timeoutMs = 35000)
{
    using var process = new Process
    {
        StartInfo = new ProcessStartInfo
        {
            FileName = executablePath,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
        },
    };
    foreach (var argument in arguments)
        process.StartInfo.ArgumentList.Add(argument);
    process.StartInfo.Environment["PATH"] = Path.GetDirectoryName(executablePath) ?? "";

    Assert(process.Start(), $"failed to start {executablePath}");
    var stdout = process.StandardOutput.ReadToEndAsync();
    var stderr = process.StandardError.ReadToEndAsync();
    try
    {
        if (!process.WaitForExit(timeoutMs))
        {
            try { process.Kill(entireProcessTree: true); } catch { }
            try { process.WaitForExit(2000); } catch { }
            throw new InvalidOperationException(
                $"process did not exit within {timeoutMs} ms: {executablePath}");
        }
        return (process.ExitCode, stdout.GetAwaiter().GetResult(), stderr.GetAwaiter().GetResult());
    }
    finally
    {
        try
        {
            if (!process.HasExited) process.Kill(entireProcessTree: true);
        }
        catch
        {
            // Best effort after a failed assertion; never leak a test child.
        }
    }
}

static StringComparer GetPathComparer() =>
    OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal;

static StringComparison GetPathComparison() =>
    OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}
