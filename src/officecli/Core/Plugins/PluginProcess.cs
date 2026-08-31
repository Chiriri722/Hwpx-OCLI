// Copyright 2026 OfficeCLI (https://OfficeCLI.AI)
// SPDX-License-Identifier: Apache-2.0

using System.Diagnostics;
using System.Text;

namespace OfficeCli.Core.Plugins;

/// <summary>
/// Shared subprocess-driver for short-lived plugin invocations
/// (dump-reader, exporter) — and for the spawn side of format-handler
/// sessions. Implements the §5.6 idle-timeout watchdog: any byte on stdout,
/// or a heartbeat line on stderr matching <c>{"heartbeat":true}</c>, resets
/// the activity timer; once the gap exceeds the budget, the process tree
/// is killed and the caller sees <c>plugin_idle_timeout</c>.
///
/// Wall-clock time is intentionally not bounded — a 4 GB .doc that takes
/// 20 minutes to dump but is constantly producing output is fine.
/// </summary>
public static class PluginProcess
{
    public sealed record RunResult(
        int ExitCode,
        string Stderr,
        bool IdleTimedOut,
        bool StdoutObserved,
        Exception? StdoutCallbackError);

    private sealed class ActivityState
    {
        public long LastActivityTicks;
        public int StdoutObserved;
        public Exception? StdoutCallbackError;
    }

    public sealed class RunOptions
    {
        public required string ExecutablePath { get; init; }
        public required IEnumerable<string> Arguments { get; init; }

        /// <summary>Idle timeout in seconds. 0 disables the watchdog entirely.</summary>
        public int IdleTimeoutSeconds { get; init; } = 60;

        /// <summary>Extra environment variables. <c>OFFICECLI_BIN</c> is added automatically.</summary>
        public Dictionary<string, string>? ExtraEnv { get; init; }

        /// <summary>
        /// Per-line stdout callback. If null, stdout is drained silently. Lines
        /// are delivered without the trailing newline. Callback exceptions are
        /// captured into <see cref="RunResult.StdoutCallbackError"/> and stop the run.
        /// </summary>
        public Action<string>? OnStdoutLine { get; init; }

        /// <summary>
        /// Optional sink for stderr lines that are not heartbeats. If null,
        /// stderr is collected into <see cref="RunResult.Stderr"/> for the
        /// caller to surface on failure.
        /// </summary>
        public Action<string>? OnStderrLine { get; init; }
    }

    public static RunResult Run(RunOptions opts)
    {
        var psi = new ProcessStartInfo
        {
            FileName = opts.ExecutablePath,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (var a in opts.Arguments) psi.ArgumentList.Add(a);

        var selfPath = Environment.ProcessPath;
        if (!string.IsNullOrEmpty(selfPath))
            psi.Environment["OFFICECLI_BIN"] = selfPath;
        if (opts.ExtraEnv is not null)
            foreach (var kv in opts.ExtraEnv)
                psi.Environment[kv.Key] = kv.Value;

        using var p = new Process { StartInfo = psi };
        if (!p.Start())
            return new RunResult(-1, "Failed to start plugin process.", false, false, null);

        // Activity timestamp shared by both reader tasks and the watchdog.
        // Wall-clock ticks (DateTime.UtcNow.Ticks, 100ns resolution) instead
        // of Stopwatch.GetTimestamp: Stopwatch is monotonic and on some
        // platform / hardware combinations keeps ticking through system
        // suspend, on others it pauses — behavior depends on QPC / TSC
        // properties. Wall-clock unambiguously advances during suspend, so
        // a laptop waking up after an hour mid-plugin gets an honest
        // "idle for an hour" reading and we kill the (likely-stale) plugin
        // instead of letting it run on dead network sockets / file handles.
        var activity = new ActivityState
        {
            LastActivityTicks = DateTime.UtcNow.Ticks,
        };
        var stderrCollector = new StringBuilder();
        var stderrLock = new object();

        var stdoutTask = Task.Run(() => ReadStdout(p, opts, activity));
        var stderrTask = Task.Run(() => ReadStderr(p, opts, stderrCollector, stderrLock, activity));
        var readerTasks = Task.WhenAll(stdoutTask, stderrTask);

        bool idleTimedOut = false;
        if (opts.IdleTimeoutSeconds > 0)
        {
            var budgetTicks = TimeSpan.FromSeconds(opts.IdleTimeoutSeconds).Ticks;
            var pollIntervalMs = Math.Max(250, opts.IdleTimeoutSeconds * 1000 / 4);

            // A process exit does not prove redirected output has drained. The
            // reader may still be delivering buffered JSONL to its callback,
            // or a descendant may have inherited a pipe handle. Keep the same
            // inactivity budget active until both readers complete so callers
            // can never replay a silently truncated stream.
            while (!p.HasExited || !readerTasks.IsCompleted)
            {
                if (p.HasExited)
                {
                    try { readerTasks.Wait(pollIntervalMs); } catch { }
                }
                else
                {
                    try { p.WaitForExit(pollIntervalMs); } catch { }
                }
                if (p.HasExited && readerTasks.IsCompleted) break;

                var since = DateTime.UtcNow.Ticks - Volatile.Read(ref activity.LastActivityTicks);
                if (since > budgetTicks)
                {
                    idleTimedOut = true;
                    try { p.Kill(entireProcessTree: true); } catch { }
                    try { p.StandardOutput.Close(); } catch { }
                    try { p.StandardError.Close(); } catch { }
                    break;
                }
            }
        }
        else
        {
            // Disabling the watchdog also disables the output-drain deadline.
            // Completion is still mandatory before a successful result.
            try { p.WaitForExit(); } catch { }
            try { readerTasks.Wait(); } catch { }
        }

        // Reap the process. After an idle timeout a callback itself may still
        // be blocked in caller code, so retain only a bounded cleanup wait; the
        // IdleTimedOut result makes every production caller fail closed.
        try { p.WaitForExit(); } catch { }
        if (idleTimedOut)
        {
            try { readerTasks.Wait(2000); } catch { }
        }

        string stderr;
        lock (stderrLock) stderr = stderrCollector.ToString();

        return new RunResult(
            p.ExitCode,
            stderr,
            idleTimedOut,
            Volatile.Read(ref activity.StdoutObserved) != 0,
            activity.StdoutCallbackError);
    }

    private static void ReadStdout(
        Process p,
        RunOptions opts,
        ActivityState activity)
    {
        using var reader = new StreamReader(
            new ActivityReadStream(p.StandardOutput.BaseStream, activity),
            new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: false),
            detectEncodingFromByteOrderMarks: false,
            bufferSize: 4096,
            leaveOpen: true);
        try
        {
            string? line;
            while ((line = reader.ReadLine()) is not null)
            {
                if (opts.OnStdoutLine is null) continue;
                try { opts.OnStdoutLine(line); }
                catch (Exception ex)
                {
                    Interlocked.CompareExchange(ref activity.StdoutCallbackError, ex, null);
                    try { p.Kill(entireProcessTree: true); } catch { }
                    return;
                }
            }
        }
        catch { /* stream closed / process killed */ }
    }

    private static void ReadStderr(Process p, RunOptions opts, StringBuilder collector, object collectorLock, ActivityState activity)
    {
        var reader = p.StandardError;
        try
        {
            string? line;
            while ((line = reader.ReadLine()) is not null)
            {
                // Heartbeat lines reset the watchdog but are NOT surfaced to
                // the caller — they're plumbing, not diagnostics. Match
                // tolerantly: any JSON object that has a truthy
                // "heartbeat" field.
                if (IsHeartbeat(line))
                {
                    Volatile.Write(ref activity.LastActivityTicks, DateTime.UtcNow.Ticks);
                    continue;
                }

                // Any non-heartbeat stderr output also counts as activity.
                Volatile.Write(ref activity.LastActivityTicks, DateTime.UtcNow.Ticks);
                if (opts.OnStderrLine is not null)
                {
                    try { opts.OnStderrLine(line); } catch { /* ignore sink errors */ }
                }
                else
                {
                    lock (collectorLock)
                    {
                        collector.AppendLine(line);
                        // Cap collected stderr at 16 KB to bound memory if a
                        // plugin spams diagnostics. We keep the head — usually
                        // the first error line is the most useful.
                        if (collector.Length > 16 * 1024)
                            collector.Length = 16 * 1024;
                    }
                }
            }
        }
        catch { /* stream closed / process killed */ }
    }

    private sealed class ActivityReadStream(Stream inner, ActivityState activity) : Stream
    {
        public override bool CanRead => inner.CanRead;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position
        {
            get => throw new NotSupportedException();
            set => throw new NotSupportedException();
        }

        public override int Read(byte[] buffer, int offset, int count)
        {
            var read = inner.Read(buffer, offset, count);
            Observe(read);
            return read;
        }

        public override int Read(Span<byte> buffer)
        {
            var read = inner.Read(buffer);
            Observe(read);
            return read;
        }

        public override int ReadByte()
        {
            var value = inner.ReadByte();
            Observe(value < 0 ? 0 : 1);
            return value;
        }

        private void Observe(int count)
        {
            if (count <= 0) return;
            Volatile.Write(ref activity.StdoutObserved, 1);
            Volatile.Write(ref activity.LastActivityTicks, DateTime.UtcNow.Ticks);
        }

        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    /// <summary>
    /// True if <paramref name="line"/> is a §5.6 heartbeat envelope
    /// (<c>{"heartbeat":true,...}</c>). Exposed so long-running drivers
    /// (FormatHandlerSession) can apply the same activity semantics on
    /// stderr without duplicating the parse.
    /// </summary>
    internal static bool IsHeartbeat(string line)
    {
        // Cheap pre-filter to avoid JSON parse cost on every diagnostic line:
        // every heartbeat envelope starts with `{` and contains "heartbeat".
        if (line.Length < 14) return false;
        if (line[0] != '{') return false;
        if (line.IndexOf("heartbeat", StringComparison.Ordinal) < 0) return false;
        try
        {
            using var doc = System.Text.Json.JsonDocument.Parse(line);
            if (doc.RootElement.ValueKind != System.Text.Json.JsonValueKind.Object) return false;
            if (!doc.RootElement.TryGetProperty("heartbeat", out var hb)) return false;
            return hb.ValueKind == System.Text.Json.JsonValueKind.True;
        }
        catch { return false; }
    }
}
