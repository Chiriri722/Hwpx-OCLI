// Copyright 2026 OfficeCLI (https://OfficeCLI.AI)
// SPDX-License-Identifier: Apache-2.0

using System.Diagnostics;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Xml.Linq;
using DocumentFormat.OpenXml.Packaging;
using DocumentFormat.OpenXml.Wordprocessing;
using OfficeCli.Core;
using OfficeCli.Core.Plugins;

if (args is ["--probe-plugin-stdout", var probeLine])
{
    Console.Out.WriteLine(probeLine);
    return 0;
}

if (args is ["dump", var directSource]
    && Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET") is { Length: > 0 } directTarget)
{
    var directSibling = Path.ChangeExtension(directSource, directTarget);
    var directMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE");
    if (directMode != "no-sibling")
    {
        File.Copy(directSource, directSibling, overwrite: true);
        File.SetLastWriteTimeUtc(directSibling, File.GetLastWriteTimeUtc(directSource));
    }
    switch (directMode)
    {
        case "whitespace":
            Console.Out.Write(" ");
            Console.Out.Flush();
            return 0;
        case "bom-only":
            Console.OpenStandardOutput().Write(new byte[] { 0xEF, 0xBB, 0xBF });
            return 0;
        case "json-and-sibling":
            Console.Out.WriteLine("{}");
            return 0;
        case "fail-after-sibling":
            return 2;
        case "foreign-sibling-on-failure":
            File.WriteAllText(directSibling, "independently published native file");
            return 2;
        default:
            return 0;
    }
}

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

if (args is ["open", _]
    && Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG") is { Length: > 0 } wireLog)
{
    var advertisedCommands = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS")
        ?? "[\"get\",\"save\"]";
    using var log = new StreamWriter(wireLog, append: false, new UTF8Encoding(false))
    {
        AutoFlush = true,
        NewLine = "\n",
    };

    while (Console.ReadLine() is { } line)
    {
        log.WriteLine(line);
        using var frame = JsonDocument.Parse(line);
        var msgType = frame.RootElement.GetProperty("msg_type").GetString();
        if (msgType == "open")
        {
            Console.WriteLine(
                "{\"protocol\":1,\"msg_type\":\"ok\",\"result\":{" +
                "\"capabilities\":{\"commands\":" + advertisedCommands + ",\"features\":[]}," +
                "\"vocabulary\":{\"addable_types\":[],\"settable_props\":{},\"path_segments\":[]}}}");
        }
        else
        {
            Console.WriteLine("{\"protocol\":1,\"msg_type\":\"ok\",\"result\":null}");
        }
        Console.Out.Flush();
        if (msgType == "close") return 0;
    }
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
    ("plugin process callback errors remain isolated per concurrent run", PluginProcessCallbackErrorsArePerRun),
    ("plugin process never reports success before output readers drain", PluginProcessWaitsForOutputReaders),
    ("format-handler lifecycle frames match protocol v1", FormatHandlerLifecycleFramesMatchProtocolV1),
    ("format-handler view uses the protocol max_lines key", FormatHandlerViewUsesProtocolMaxLinesKey),
    ("format-handler save cannot report false durability", FormatHandlerSaveCannotReportFalseDurability),
    ("dump-reader accepts direct native output without a blank warning", DumpReaderDirectNativeOutputIsNotWarned),
    ("dump-reader direct native mode is exclusive", DumpReaderDirectNativeProtocolIsExclusive),
    ("dump-reader failure never deletes a matching published sibling", DumpReaderDirectNativeFailurePreservesMatchingSibling),
    ("dump-reader failure preserves an unowned concurrent sibling", DumpReaderDirectNativeFailurePreservesUnownedSibling),
    ("dump-reader rejects a different preexisting direct-native sibling before launch", DumpReaderDirectNativePreexistingConflictIsPreserved),
    ("dump-reader detects mutation of an identical preexisting direct-native sibling", DumpReaderDirectNativePreexistingMutationIsRejected),
    ("dump-reader surfaces only bounded structured success warnings", DumpReaderStructuredWarningsAreFilteredAndBounded),
    ("field schema accepts emitted character formatting", FieldSchemaAcceptsEmittedCharacterFormatting),
    ("style schema accepts emitted paragraph indents", StyleSchemaAcceptsEmittedParagraphIndents),
    ("style add preserves numeric ids and forward next references", StyleAddPreservesNumericIdsAndForwardNextReferences),
    ("note reference decorations preserve prefix suffix and baseline", NoteReferenceDecorationsArePreserved),
    ("raw chart parts preserve values and normalize schema order", RawChartPartsPreserveValuesAndNormalizeSchemaOrder),
    ("raw chart carrier rejects unsafe payloads atomically", RawChartCarrierRejectsUnsafePayloadsAtomically),
    ("textbox and shape preserve inline anchor layout contracts", TextboxAndShapePreserveInlineAnchorLayoutContracts),
    ("textbox ordinals include nested cell drawings", TextboxOrdinalsIncludeNestedCellDrawings),
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

static void PluginProcessCallbackErrorsArePerRun()
{
    using var firstEntered = new ManualResetEventSlim();
    using var secondEntered = new ManualResetEventSlim();
    using var allowFirstFailure = new ManualResetEventSlim();
    using var allowSecondCompletion = new ManualResetEventSlim();

    var firstTask = Task.Run(() => PluginProcess.Run(new PluginProcess.RunOptions
    {
        ExecutablePath = TestAppHostPath(),
        Arguments = new[] { "--probe-plugin-stdout", "first" },
        IdleTimeoutSeconds = 5,
        OnStdoutLine = _ =>
        {
            firstEntered.Set();
            Assert(allowFirstFailure.Wait(TimeSpan.FromSeconds(5)), "first callback release timed out");
            throw new InvalidOperationException("first callback failed");
        },
    }));
    Assert(firstEntered.Wait(TimeSpan.FromSeconds(5)), "first callback did not start");

    var secondTask = Task.Run(() => PluginProcess.Run(new PluginProcess.RunOptions
    {
        ExecutablePath = TestAppHostPath(),
        Arguments = new[] { "--probe-plugin-stdout", "second" },
        IdleTimeoutSeconds = 5,
        OnStdoutLine = _ =>
        {
            secondEntered.Set();
            Assert(allowSecondCompletion.Wait(TimeSpan.FromSeconds(5)), "second callback release timed out");
        },
    }));
    Assert(secondEntered.Wait(TimeSpan.FromSeconds(5)), "second callback did not start");

    allowFirstFailure.Set();
    Assert(firstTask.Wait(TimeSpan.FromSeconds(5)), "first plugin run did not finish");
    allowSecondCompletion.Set();
    Assert(secondTask.Wait(TimeSpan.FromSeconds(5)), "second plugin run did not finish");

    var first = firstTask.Result;
    var second = secondTask.Result;
    Assert(first.StdoutObserved && second.StdoutObserved, "raw stdout activity was not observed");
    Assert(first.StdoutCallbackError?.Message == "first callback failed",
        "first callback error was lost or replaced");
    Assert(second.StdoutCallbackError is null,
        $"second run inherited another run's callback error: {second.StdoutCallbackError}");
}

static void PluginProcessWaitsForOutputReaders()
{
    using var callbackEntered = new ManualResetEventSlim();
    using var releaseCallback = new ManualResetEventSlim();
    Task<PluginProcess.RunResult>? runTask = null;

    try
    {
        runTask = Task.Run(() => PluginProcess.Run(new PluginProcess.RunOptions
        {
            ExecutablePath = TestAppHostPath(),
            Arguments = new[] { "--probe-plugin-stdout", "slow-reader" },
            IdleTimeoutSeconds = 1,
            OnStdoutLine = _ =>
            {
                callbackEntered.Set();
                releaseCallback.Wait();
            },
        }));

        Assert(callbackEntered.Wait(TimeSpan.FromSeconds(5)),
            "slow stdout callback did not start");
        Assert(runTask.Wait(TimeSpan.FromSeconds(5)),
            "plugin runner did not apply its idle budget to an incomplete output reader");
        Assert(runTask.Result.IdleTimedOut,
            "plugin runner reported success while its stdout callback was still incomplete");
    }
    finally
    {
        releaseCallback.Set();
        if (runTask is not null)
            Assert(runTask.Wait(TimeSpan.FromSeconds(5)),
                "plugin runner did not finish after the callback was released");
    }
}

static void FormatHandlerLifecycleFramesMatchProtocolV1()
{
    var documentPath = Path.Combine(Path.GetTempPath(), $"officecli-format-wire-{Guid.NewGuid():N}.wire");
    var wireLog = Path.Combine(Path.GetTempPath(), $"officecli-format-wire-{Guid.NewGuid():N}.jsonl");
    var originalLog = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG");
    var originalCommands = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS");
    try
    {
        File.WriteAllText(documentPath, "wire contract");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", wireLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", "[\"get\",\"save\"]");

        using (var handler = OpenContractFormatHandler(documentPath))
            handler.Save();

        var frames = File.ReadAllLines(wireLog);
        Assert(frames.Length == 3, $"expected open/save/close frames, got {frames.Length}");

        using var open = JsonDocument.Parse(frames[0]);
        var openRoot = open.RootElement;
        Assert(openRoot.GetProperty("msg_type").GetString() == "open", "first frame is not open");
        Assert(openRoot.TryGetProperty("path", out var path)
               && path.GetString() == Path.GetFullPath(documentPath),
            "open frame does not carry the canonical top-level path");
        Assert(openRoot.TryGetProperty("editable", out var editable) && editable.GetBoolean(),
            "open frame does not carry the canonical top-level editable flag");
        Assert(!openRoot.TryGetProperty("args", out _), "open lifecycle fields were nested under args");

        using var save = JsonDocument.Parse(frames[1]);
        var saveRoot = save.RootElement;
        Assert(saveRoot.GetProperty("msg_type").GetString() == "save", "second frame is not save");
        Assert(!saveRoot.TryGetProperty("command", out _), "save was sent as a command envelope");
        Assert(!saveRoot.TryGetProperty("args", out _), "save lifecycle frame carried command args");

        using var close = JsonDocument.Parse(frames[2]);
        Assert(close.RootElement.GetProperty("msg_type").GetString() == "close", "third frame is not close");
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", originalLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", originalCommands);
        File.Delete(documentPath);
        File.Delete(wireLog);
    }
}

static void FormatHandlerViewUsesProtocolMaxLinesKey()
{
    var documentPath = Path.Combine(Path.GetTempPath(), $"officecli-format-view-{Guid.NewGuid():N}.wire");
    var wireLog = Path.Combine(Path.GetTempPath(), $"officecli-format-view-{Guid.NewGuid():N}.jsonl");
    var originalLog = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG");
    var originalCommands = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS");
    try
    {
        File.WriteAllText(documentPath, "wire contract");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", wireLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", "[\"view\"]");

        using (var handler = OpenContractFormatHandler(documentPath))
            _ = handler.ViewAsText(maxLines: 1);

        var frames = File.ReadAllLines(wireLog);
        Assert(frames.Length == 3, $"expected open/view/close frames, got {frames.Length}");
        using var view = JsonDocument.Parse(frames[1]);
        var args = view.RootElement.GetProperty("args");
        Assert(args.GetProperty("max_lines").GetInt32() == 1,
            "view frame did not use protocol max_lines");
        Assert(!args.TryGetProperty("max-lines", out _),
            "view frame leaked the CLI-only --max-lines spelling into the plugin protocol");
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", originalLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", originalCommands);
        File.Delete(documentPath);
        File.Delete(wireLog);
    }
}

static void FormatHandlerSaveCannotReportFalseDurability()
{
    var documentPath = Path.Combine(Path.GetTempPath(), $"officecli-format-nosave-{Guid.NewGuid():N}.wire");
    var wireLog = Path.Combine(Path.GetTempPath(), $"officecli-format-nosave-{Guid.NewGuid():N}.jsonl");
    var originalLog = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG");
    var originalCommands = Environment.GetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS");
    try
    {
        File.WriteAllText(documentPath, "wire contract");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", wireLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", "[\"get\"]");

        using var handler = OpenContractFormatHandler(documentPath);
        try
        {
            handler.Save();
            throw new InvalidOperationException("Save succeeded although the plugin omitted the save capability");
        }
        catch (CliException ex)
        {
            Assert(ex.Code == "unsupported_command", $"unexpected missing-save error code: {ex.Code}");
        }
    }
    finally
    {
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_WIRE_LOG", originalLog);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_FORMAT_HANDLER_COMMANDS", originalCommands);
        File.Delete(documentPath);
        File.Delete(wireLog);
    }
}

static void StyleAddPreservesNumericIdsAndForwardNextReferences()
{
    var path = Path.Combine(Path.GetTempPath(), $"officecli-style-forward-next-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(path);
        using (var handler = new OfficeCli.Handlers.WordHandler(path, editable: true))
        {
            var firstPath = handler.Add("/styles", "style", null, new Dictionary<string, string>
            {
                ["id"] = "7",
                ["name"] = "Hancom Child",
                ["type"] = "paragraph",
                ["next"] = "0",
                ["customStyle"] = "false",
            });
            Assert(firstPath == "/styles/7", $"numeric style id was rewritten: {firstPath}");
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"forward-next style props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");

            var secondPath = handler.Add("/styles", "style", null, new Dictionary<string, string>
            {
                ["id"] = "0",
                ["name"] = "Hancom Root",
                ["type"] = "paragraph",
                ["customStyle"] = "false",
            });
            Assert(secondPath == "/styles/0", $"numeric target style id was rewritten: {secondPath}");
            handler.Save();
        }

        using var document = WordprocessingDocument.Open(path, false);
        var styles = document.MainDocumentPart!.StyleDefinitionsPart!.Styles!;
        var child = styles.Elements<Style>().Single(style => style.StyleId?.Value == "7");
        Assert(child.NextParagraphStyle?.Val?.Value == "0",
            $"forward next reference changed: {child.NextParagraphStyle?.Val?.Value ?? "<missing>"}");
        Assert(styles.Elements<Style>().Any(style => style.StyleId?.Value == "0"),
            "numeric target style was not preserved");
    }
    finally
    {
        if (File.Exists(path)) File.Delete(path);
    }
}

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

static void TextboxAndShapePreserveInlineAnchorLayoutContracts()
{
    var path = Path.Combine(Path.GetTempPath(), $"officecli-drawing-layout-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(path);
        using (var handler = new OfficeCli.Handlers.WordHandler(path, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "before",
            });

            var inlineTextbox = handler.Add("/body/p[1]", "textbox", null, new Dictionary<string, string>
            {
                ["anchor"] = "false",
                ["width"] = "1270000emu",
                ["height"] = "635000emu",
                ["wrapDist"] = "11,22,33,44",
                ["geometry"] = "roundRect",
                ["cornerRadius"] = "20",
                ["text"] = "inline textbox",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"inline textbox props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(inlineTextbox == "/body/textbox[1]", $"unexpected inline textbox path: {inlineTextbox}");

            var inlineShape = handler.Add("/body/p[1]", "shape", null, new Dictionary<string, string>
            {
                ["anchor"] = "false",
                ["width"] = "254000emu",
                ["height"] = "381000emu",
                ["wrapDist"] = "55,66,77,88",
                ["fill"] = "FFFFFF",
                ["line.style"] = "none",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"inline shape props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(inlineShape == "/body/shape[1]", $"unexpected inline shape path: {inlineShape}");

            var floatingTextbox = handler.Add("/body/p[1]", "textbox", null, new Dictionary<string, string>
            {
                ["anchor"] = "true",
                ["width"] = "1524000emu",
                ["height"] = "762000emu",
                ["anchor.x"] = "101emu",
                ["anchor.y"] = "202emu",
                ["hRelative"] = "page",
                ["vRelative"] = "page",
                ["wrap"] = "through",
                ["wrap.side"] = "right",
                ["wrapDist"] = "111,222,333,444",
                ["behindDoc"] = "true",
                ["allowOverlap"] = "false",
                ["relativeHeight"] = "77",
                ["description"] = "도형 설명 & 접근성\n둘째 줄",
                ["text"] = "floating textbox",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"floating textbox props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(floatingTextbox == "/body/textbox[2]", $"unexpected floating textbox path: {floatingTextbox}");

            var floatingShape = handler.Add("/body/p[1]", "shape", null, new Dictionary<string, string>
            {
                ["anchor"] = "true",
                ["width"] = "508000emu",
                ["height"] = "508000emu",
                ["anchor.x"] = "303emu",
                ["anchor.y"] = "404emu",
                ["hRelative"] = "page",
                ["vRelative"] = "page",
                ["wrap"] = "topAndBottom",
                ["wrapDist"] = "555,666,777,888",
                ["behindDoc"] = "false",
                ["relativeHeight"] = "88",
                ["allowOverlap"] = "false",
                ["geometry"] = "roundRect",
                ["cornerRadius"] = "35",
                ["fill"] = "102030",
                ["line.color"] = "405060",
                ["line.width"] = "12700emu",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"floating shape props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(floatingShape == "/body/shape[2]", $"unexpected floating shape path: {floatingShape}");

            var centeredEllipse = handler.Add("/body/p[1]", "shape", null, new Dictionary<string, string>
            {
                ["anchor"] = "true",
                ["width"] = "852170emu",
                ["height"] = "804291emu",
                ["hAlign"] = "center",
                ["anchor.y"] = "9057513emu",
                ["hRelative"] = "page",
                ["vRelative"] = "page",
                ["wrap"] = "none",
                ["allowOverlap"] = "false",
                ["relativeHeight"] = "9",
                ["description"] = "타원입니다.",
                ["geometry"] = "ellipse",
                ["fill"] = "none",
                ["line.color"] = "FF0000",
                ["line.width"] = "0emu",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"centered ellipse props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(centeredEllipse == "/body/shape[3]", $"unexpected centered ellipse path: {centeredEllipse}");

            var legacyBehindShape = handler.Add("/body/p[1]", "shape", null, new Dictionary<string, string>
            {
                ["anchor"] = "true",
                ["width"] = "127000emu",
                ["height"] = "127000emu",
                ["wrap"] = "behind",
                ["geometry"] = "rect",
                ["fill"] = "AABBCC",
            });
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"legacy behind shape props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            Assert(legacyBehindShape == "/body/shape[4]", $"unexpected legacy behind shape path: {legacyBehindShape}");
            handler.Save();
        }

        using (var source = new OfficeCli.Handlers.WordHandler(path, editable: false))
        {
            var items = OfficeCli.Handlers.WordBatchEmitter.EmitWord(source);
            var textboxes = items
                .Where(item => item.Command == "add" && item.Type == "textbox")
                .ToList();
            Assert(textboxes.Count == 2, $"expected two typed textbox rows, got {textboxes.Count}");
            Assert(textboxes[0].Props?.GetValueOrDefault("anchor") == "false",
                "inline textbox dump did not preserve anchor=false");
            Assert(textboxes[0].Props?.GetValueOrDefault("wrapDist") == "11,22,33,44",
                "inline textbox dump did not preserve wrap distances");
            Assert(textboxes[0].Props?.GetValueOrDefault("geometry") == "roundRect"
                && textboxes[0].Props?.GetValueOrDefault("cornerRadius") == "20",
                "inline textbox dump did not preserve the rounded-rectangle adjustment");
            Assert(textboxes[1].Props?.GetValueOrDefault("anchor") == "true",
                "floating textbox dump did not preserve anchor=true");
            Assert(textboxes[1].Props?.GetValueOrDefault("wrap") == "through"
                && textboxes[1].Props?.GetValueOrDefault("wrap.side") == "right",
                "floating textbox dump did not preserve through/right wrapping");
            Assert(textboxes[1].Props?.GetValueOrDefault("wrapDist") == "111,222,333,444",
                "floating textbox dump did not preserve wrap distances");
            Assert(textboxes[1].Props?.GetValueOrDefault("behindDoc") == "true",
                "floating textbox dump did not preserve behindDoc");
            Assert(textboxes[1].Props?.GetValueOrDefault("allowOverlap") == "false",
                "floating textbox dump did not preserve allowOverlap=false");
            Assert(textboxes[1].Props?.GetValueOrDefault("relativeHeight") == "77",
                "floating textbox dump did not preserve z-order");
            Assert(textboxes[1].Props?.GetValueOrDefault("description") == "도형 설명 & 접근성\n둘째 줄",
                "floating textbox dump did not preserve its object description");

            var shapes = items
                .Where(item => item.Command == "raw-set"
                    && item.Xml?.Contains("<wps:wsp", StringComparison.Ordinal) == true
                    && item.Xml.Contains("<wps:txbx", StringComparison.Ordinal) == false)
                .ToList();
            Assert(shapes.Count == 4, $"expected four lossless shape carriers, got {shapes.Count}");
            var centeredEllipseCarrier = shapes.Single(item =>
                item.Xml!.Contains("prst=\"ellipse\"", StringComparison.Ordinal));
            Assert(centeredEllipseCarrier.Xml!.Contains(
                    "<wp:positionH relativeFrom=\"page\"><wp:align>center</wp:align>",
                    StringComparison.Ordinal),
                "centered ellipse carrier did not preserve alignment in place of an X offset");
            Assert(centeredEllipseCarrier.Xml.Contains("allowOverlap=\"0\"", StringComparison.Ordinal)
                && centeredEllipseCarrier.Xml.Contains("<a:noFill", StringComparison.Ordinal),
                "centered ellipse carrier did not preserve no-fill/overlap metadata");
        }

        using var document = WordprocessingDocument.Open(path, false);
        var paragraphs = document.MainDocumentPart!.Document!.Body!.Elements<Paragraph>().ToList();
        Assert(paragraphs.Count == 1, $"drawings created extra host paragraphs: {paragraphs.Count}");
        var drawings = paragraphs[0].Descendants<Drawing>().ToList();
        Assert(drawings.Count == 6, $"expected six drawings in one paragraph, got {drawings.Count}");

        var inlineXml = drawings[0].OuterXml;
        Assert(inlineXml.Contains("<wp:inline", StringComparison.Ordinal)
            && inlineXml.Contains("distT=\"11\"", StringComparison.Ordinal)
            && inlineXml.Contains("distB=\"22\"", StringComparison.Ordinal)
            && inlineXml.Contains("distL=\"33\"", StringComparison.Ordinal)
            && inlineXml.Contains("distR=\"44\"", StringComparison.Ordinal)
            && inlineXml.Contains("prst=\"roundRect\"", StringComparison.Ordinal)
            && inlineXml.Contains("name=\"adj\" fmla=\"val 20000\"", StringComparison.Ordinal),
            "inline textbox did not preserve inline placement or distances");

        var inlineShapeXml = drawings[1].OuterXml;
        Assert(inlineShapeXml.Contains("<wp:inline", StringComparison.Ordinal)
            && !inlineShapeXml.Contains("<wps:txbx", StringComparison.Ordinal)
            && inlineShapeXml.Contains("distT=\"55\"", StringComparison.Ordinal)
            && inlineShapeXml.Contains("distR=\"88\"", StringComparison.Ordinal),
            "inline shape did not preserve inline placement or distances");

        var floatingTextboxXml = drawings[2].OuterXml;
        Assert(floatingTextboxXml.Contains("<wp:anchor", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("relativeHeight=\"77\"", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("behindDoc=\"1\"", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("allowOverlap=\"0\"", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("distT=\"111\"", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("distR=\"444\"", StringComparison.Ordinal)
            && floatingTextboxXml.Contains("<wp:wrapThrough wrapText=\"right\"", StringComparison.Ordinal),
            "floating textbox did not preserve anchor layout metadata");

        var floatingShapeXml = drawings[3].OuterXml;
        Assert(floatingShapeXml.Contains("<wp:anchor", StringComparison.Ordinal)
            && floatingShapeXml.Contains("relativeHeight=\"88\"", StringComparison.Ordinal)
            && floatingShapeXml.Contains("allowOverlap=\"0\"", StringComparison.Ordinal)
            && floatingShapeXml.Contains("distT=\"555\"", StringComparison.Ordinal)
            && floatingShapeXml.Contains("distR=\"888\"", StringComparison.Ordinal)
            && floatingShapeXml.Contains("<wp:wrapTopAndBottom", StringComparison.Ordinal)
            && floatingShapeXml.Contains("prst=\"roundRect\"", StringComparison.Ordinal)
            && floatingShapeXml.Contains("name=\"adj\" fmla=\"val 35000\"", StringComparison.Ordinal),
            "floating shape did not preserve anchor layout metadata");

        var centeredEllipseXml = drawings[4].OuterXml;
        Assert(centeredEllipseXml.Contains("<wp:positionH relativeFrom=\"page\"><wp:align>center</wp:align>", StringComparison.Ordinal)
            && centeredEllipseXml.Contains("<wp:positionV relativeFrom=\"page\"><wp:posOffset>9057513</wp:posOffset>", StringComparison.Ordinal)
            && centeredEllipseXml.Contains("allowOverlap=\"0\"", StringComparison.Ordinal)
            && centeredEllipseXml.Contains("prst=\"ellipse\"", StringComparison.Ordinal)
            && centeredEllipseXml.Contains("descr=\"타원입니다.\"", StringComparison.Ordinal)
            && centeredEllipseXml.Contains("<a:noFill", StringComparison.Ordinal),
            "centered ellipse did not preserve semantic page alignment or no-fill geometry");

        var legacyBehindShapeXml = drawings[5].OuterXml;
        Assert(legacyBehindShapeXml.Contains("<wp:wrapNone", StringComparison.Ordinal)
            && legacyBehindShapeXml.Contains("behindDoc=\"1\"", StringComparison.Ordinal),
            "legacy wrap=behind did not place the shape behind document text");
    }
    finally
    {
        if (File.Exists(path)) File.Delete(path);
    }
}

static void RawChartPartsPreserveValuesAndNormalizeSchemaOrder()
{
    var sourcePath = Path.Combine(Path.GetTempPath(), $"officecli-chart-carrier-source-{Guid.NewGuid():N}.docx");
    var targetPath = Path.Combine(Path.GetTempPath(), $"officecli-chart-carrier-target-{Guid.NewGuid():N}.docx");
    var normalizedPath = Path.Combine(Path.GetTempPath(), $"officecli-chart-carrier-normalized-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(sourcePath);
        using (var handler = new OfficeCli.Handlers.WordHandler(sourcePath, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "source",
            });
            handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
            {
                ["type"] = "column3d",
                ["data"] = "Series 1:1,2,3",
                ["categories"] = "A,B,C",
                ["title"] = "Carrier source",
            });
            handler.Save();
        }

        string sourceChartXml;
        using (var source = WordprocessingDocument.Open(sourcePath, false))
        {
            sourceChartXml = source.MainDocumentPart!.ChartParts.Single().ChartSpace!.OuterXml;
        }

        OfficeCli.BlankDocCreator.Create(targetPath);
        using (var handler = new OfficeCli.Handlers.WordHandler(targetPath, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "target",
            });
            var result = handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
            {
                ["chartXmlBase64"] = Convert.ToBase64String(Encoding.UTF8.GetBytes(sourceChartXml)),
                ["width"] = "4095750emu",
                ["height"] = "2381250emu",
                ["anchor"] = "true",
                ["wrap"] = "square",
                ["hrelative"] = "column",
                ["vrelative"] = "paragraph",
                ["hposition"] = "0emu",
                ["vposition"] = "0emu",
                ["relativeHeight"] = "8",
                ["wrapDist"] = "0,0,0,0",
                ["name"] = "한컴 차트 1",
                ["description"] = "한컴 차트 설명",
            });
            Assert(result == "/chart[1]", $"unexpected raw chart path: {result}");
            Assert(handler.LastAddUnsupportedProps.Count == 0,
                $"raw chart props were rejected: {string.Join(", ", handler.LastAddUnsupportedProps)}");
            handler.Save();
        }

        using (var target = WordprocessingDocument.Open(targetPath, false))
        {
            var main = target.MainDocumentPart!;
            var targetChartPart = main.ChartParts.Single();
            var targetChartXml = targetChartPart.ChartSpace!.OuterXml;
            Assert(XNode.DeepEquals(XElement.Parse(sourceChartXml), XElement.Parse(targetChartXml)),
                "raw chart XML changed during carrier insertion");
            var chartReference = main.Document!
                .Descendants<DocumentFormat.OpenXml.Drawing.Charts.ChartReference>()
                .Single();
            Assert(ReferenceEquals(main.GetPartById(chartReference.Id!), targetChartPart),
                "document chart reference does not resolve to the inserted ChartPart");
            Assert(main.Parts.Count(pair => ReferenceEquals(pair.OpenXmlPart, targetChartPart)) == 1,
                "raw chart carrier created an ambiguous host-to-chart topology");
            Assert(!targetChartPart.Parts.Any()
                   && !targetChartPart.ExternalRelationships.Any()
                   && !targetChartPart.HyperlinkRelationships.Any()
                   && !targetChartPart.DataPartReferenceRelationships.Any(),
                "self-contained raw chart unexpectedly gained outbound relationships");

            var anchor = main.Document!.Descendants<DocumentFormat.OpenXml.Drawing.Wordprocessing.Anchor>().Single();
            Assert(anchor.Extent?.Cx?.Value == 4095750L && anchor.Extent?.Cy?.Value == 2381250L,
                "raw chart extent did not preserve HWPUNIT-to-EMU geometry");
            Assert(anchor.HorizontalPosition?.RelativeFrom?.Value
                    == DocumentFormat.OpenXml.Drawing.Wordprocessing.HorizontalRelativePositionValues.Column,
                "raw chart horizontal reference is not column-relative");
            Assert(anchor.VerticalPosition?.RelativeFrom?.Value
                    == DocumentFormat.OpenXml.Drawing.Wordprocessing.VerticalRelativePositionValues.Paragraph,
                "raw chart vertical reference is not paragraph-relative");
            Assert(anchor.HorizontalPosition?.GetFirstChild<DocumentFormat.OpenXml.Drawing.Wordprocessing.PositionOffset>()?.Text == "0",
                "raw chart horizontal offset changed");
            Assert(anchor.VerticalPosition?.GetFirstChild<DocumentFormat.OpenXml.Drawing.Wordprocessing.PositionOffset>()?.Text == "0",
                "raw chart vertical offset changed");
            Assert(anchor.RelativeHeight?.Value == 8U,
                "raw chart relative height changed");
            Assert(anchor.AllowOverlap?.Value == true && anchor.LayoutInCell?.Value == true,
                "raw chart native overlap/layout flags changed");
            Assert(anchor.DistanceFromTop?.Value == 0U && anchor.DistanceFromBottom?.Value == 0U
                && anchor.DistanceFromLeft?.Value == 0U && anchor.DistanceFromRight?.Value == 0U,
                "raw chart wrap distances changed");
            Assert(anchor.GetFirstChild<DocumentFormat.OpenXml.Drawing.Wordprocessing.WrapSquare>()?.WrapText?.Value
                    == DocumentFormat.OpenXml.Drawing.Wordprocessing.WrapTextValues.BothSides,
                "raw chart wrap mode changed");
            Assert(anchor.GetFirstChild<DocumentFormat.OpenXml.Drawing.Wordprocessing.DocProperties>()?.Name?.Value == "한컴 차트 1",
                "raw chart object name changed");
            Assert(anchor.GetFirstChild<DocumentFormat.OpenXml.Drawing.Wordprocessing.DocProperties>()?.Description?.Value == "한컴 차트 설명",
                "raw chart accessibility description changed");
        }

        var chartNs = XNamespace.Get("http://schemas.openxmlformats.org/drawingml/2006/chart");
        var noncanonicalChart = XDocument.Parse(sourceChartXml);
        foreach (var axis in noncanonicalChart.Descendants()
                     .Where(element => element.Name == chartNs + "catAx" || element.Name == chartNs + "valAx"))
        {
            var delete = axis.Element(chartNs + "delete")
                ?? new XElement(chartNs + "delete", new XAttribute("val", "0"));
            delete.Remove();
            var crossAxis = axis.Element(chartNs + "crossAx")
                ?? throw new InvalidOperationException("source chart is missing c:crossAx");
            crossAxis.AddAfterSelf(delete);
        }
        var chart = noncanonicalChart.Root?.Element(chartNs + "chart")
            ?? throw new InvalidOperationException("source chart is missing c:chart");
        var plotArea = chart.Element(chartNs + "plotArea")
            ?? throw new InvalidOperationException("source chart is missing c:plotArea");
        var view3D = chart.Element(chartNs + "view3D");
        view3D?.Remove();
        plotArea.AddBeforeSelf(new XElement(chartNs + "view3D",
            new XElement(chartNs + "rAngAx", new XAttribute("val", "1")),
            new XElement(chartNs + "rotX", new XAttribute("val", "15")),
            new XElement(chartNs + "rotY", new XAttribute("val", "20")),
            new XElement(chartNs + "perspective", new XAttribute("val", "30")),
            new XElement(chartNs + "hPercent", new XAttribute("val", "100")),
            new XElement(chartNs + "depthPercent", new XAttribute("val", "100"))));

        OfficeCli.BlankDocCreator.Create(normalizedPath);
        using (var handler = new OfficeCli.Handlers.WordHandler(normalizedPath, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "normalized",
            });
            var encodedNoncanonicalChart = Convert.ToBase64String(
                Encoding.UTF8.GetBytes(noncanonicalChart.ToString(SaveOptions.DisableFormatting)));
            var strictRejected = false;
            try
            {
                handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
                {
                    ["chartXmlBase64"] = encodedNoncanonicalChart,
                    ["width"] = "4095750emu",
                    ["height"] = "2381250emu",
                });
            }
            catch (ArgumentException)
            {
                strictRejected = true;
            }
            Assert(strictRejected,
                "the generic raw carrier silently applied Hancom compatibility normalization");

            var unprofiledAxisChart = new XDocument(noncanonicalChart);
            var unprofiledCategoryAxis = unprofiledAxisChart
                .Descendants(chartNs + "catAx")
                .First();
            unprofiledCategoryAxis.Name = chartNs + "dateAx";
            var unprofiledRejected = false;
            try
            {
                handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
                {
                    ["chartXmlBase64"] = Convert.ToBase64String(Encoding.UTF8.GetBytes(
                        unprofiledAxisChart.ToString(SaveOptions.DisableFormatting))),
                    ["chartXmlProfile"] = "hwpxChartOrderRepairV1",
                    ["width"] = "4095750emu",
                    ["height"] = "2381250emu",
                });
            }
            catch (ArgumentException)
            {
                unprofiledRejected = true;
            }
            Assert(unprofiledRejected, "Hancom v1 profile repaired an unprofiled dateAx error");

            var duplicateChart = new XDocument(noncanonicalChart);
            var duplicateAxis = duplicateChart.Descendants(chartNs + "catAx").First();
            duplicateAxis.Element(chartNs + "delete")!.AddAfterSelf(
                new XElement(chartNs + "delete", new XAttribute("val", "1")));
            var duplicateRejected = false;
            try
            {
                handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
                {
                    ["chartXmlBase64"] = Convert.ToBase64String(Encoding.UTF8.GetBytes(
                        duplicateChart.ToString(SaveOptions.DisableFormatting))),
                    ["chartXmlProfile"] = "hwpxChartOrderRepairV1",
                    ["width"] = "4095750emu",
                    ["height"] = "2381250emu",
                });
            }
            catch (ArgumentException)
            {
                duplicateRejected = true;
            }
            Assert(duplicateRejected, "Hancom v1 profile guessed between duplicate singleton children");

            handler.Add("/body/p[1]", "chart", null, new Dictionary<string, string>
            {
                ["chartXmlBase64"] = encodedNoncanonicalChart,
                ["chartXmlProfile"] = "hwpxChartOrderRepairV1",
                ["width"] = "4095750emu",
                ["height"] = "2381250emu",
            });
            handler.Save();
        }
        using (var normalized = WordprocessingDocument.Open(normalizedPath, false))
        {
            var errors = new DocumentFormat.OpenXml.Validation.OpenXmlValidator(
                    DocumentFormat.OpenXml.FileFormatVersions.Microsoft365)
                .Validate(normalized)
                .ToList();
            Assert(errors.Count == 0,
                $"schema-order normalization left validation errors: {string.Join(" | ", errors.Select(error => error.Description))}");
            var normalizedChart = XElement.Parse(
                normalized.MainDocumentPart!.ChartParts.Single().ChartSpace!.OuterXml);
            static string InfosetFingerprint(XElement root)
            {
                return string.Join("\n", root.DescendantsAndSelf().Select(element =>
                {
                    var path = string.Join("/", element.AncestorsAndSelf().Reverse()
                        .Select(ancestor => $"{{{ancestor.Name.NamespaceName}}}{ancestor.Name.LocalName}"));
                    var attributes = string.Join(";", element.Attributes()
                        .Where(attribute => !attribute.IsNamespaceDeclaration)
                        .Select(attribute => $"{{{attribute.Name.NamespaceName}}}{attribute.Name.LocalName}={attribute.Value}")
                        .OrderBy(value => value, StringComparer.Ordinal));
                    var leafText = element.HasElements ? "" : element.Value;
                    return $"{path}|{attributes}|{leafText}";
                }).OrderBy(value => value, StringComparer.Ordinal));
            }
            Assert(InfosetFingerprint(noncanonicalChart.Root!) == InfosetFingerprint(normalizedChart),
                "Hancom order repair changed chart elements, attributes, text, or parentage");
            var normalizedView3D = normalizedChart.Descendants(chartNs + "view3D").Single();
            Assert(string.Join(",", normalizedView3D.Elements().Select(element => element.Name.LocalName))
                    == "rotX,hPercent,rotY,depthPercent,rAngAx,perspective",
                "view3D children were not normalized to CT_View3D order");
            foreach (var axis in normalizedChart.Descendants()
                         .Where(element => element.Name == chartNs + "catAx" || element.Name == chartNs + "valAx"))
            {
                var children = axis.Elements().Select(element => element.Name.LocalName).ToList();
                Assert(children.IndexOf("delete") < children.IndexOf("axPos")
                       && children.IndexOf("crossAx") > children.IndexOf("tickLblPos"),
                    $"axis children were not normalized: {string.Join(",", children)}");
            }
        }

    }
    finally
    {
        if (File.Exists(sourcePath)) File.Delete(sourcePath);
        if (File.Exists(targetPath)) File.Delete(targetPath);
        if (File.Exists(normalizedPath)) File.Delete(normalizedPath);
    }
}

static void RawChartCarrierRejectsUnsafePayloadsAtomically()
{
    const string chartNs = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    const string relationshipsNs = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    var validChart = $"<c:chartSpace xmlns:c=\"{chartNs}\"><c:chart><c:plotArea><c:layout/></c:plotArea></c:chart></c:chartSpace>";
    var deepChart = validChart
        .Replace("<c:chart>", "<c:chart>" + string.Concat(Enumerable.Repeat("<c:ext>", 256)), StringComparison.Ordinal)
        .Replace("</c:chart>", string.Concat(Enumerable.Repeat("</c:ext>", 256)) + "</c:chart>", StringComparison.Ordinal);
    var cases = new (string Label, string Encoded, KeyValuePair<string, string>? ExtraProperty)[]
    {
        ("invalid base64", "not-base64", null),
        ("invalid UTF-8", Convert.ToBase64String(new byte[] { 0xc3, 0x28 }), null),
        ("DTD", Convert.ToBase64String(Encoding.UTF8.GetBytes(
            $"<!DOCTYPE c:chartSpace [<!ENTITY injected 'x'>]>{validChart}")), null),
        ("processing instruction", Convert.ToBase64String(Encoding.UTF8.GetBytes(
            validChart.Replace("<c:chart>", "<c:chart><?unsafe value?>", StringComparison.Ordinal))), null),
        ("unknown namespace", Convert.ToBase64String(Encoding.UTF8.GetBytes(
            validChart.Replace("</c:chartSpace>", "<evil:payload xmlns:evil=\"urn:unverified\"/></c:chartSpace>", StringComparison.Ordinal))), null),
        ("external relationship", Convert.ToBase64String(Encoding.UTF8.GetBytes(
            validChart.Replace("</c:chartSpace>", $"<c:externalData xmlns:r=\"{relationshipsNs}\" r:id=\"rId1\"/></c:chartSpace>", StringComparison.Ordinal))), null),
        ("excessive nesting", Convert.ToBase64String(Encoding.UTF8.GetBytes(deepChart)), null),
        ("duplicate chart", Convert.ToBase64String(Encoding.UTF8.GetBytes(
            validChart.Replace("</c:chartSpace>", "<c:chart><c:plotArea/></c:chart></c:chartSpace>", StringComparison.Ordinal))), null),
        ("unknown compatibility profile", Convert.ToBase64String(Encoding.UTF8.GetBytes(validChart)),
            new KeyValuePair<string, string>("chartXmlProfile", "other")),
        ("semantic property mixing", Convert.ToBase64String(Encoding.UTF8.GetBytes(validChart)),
            new KeyValuePair<string, string>("data", "Series 1:1")),
    };

    var path = Path.Combine(Path.GetTempPath(), $"officecli-chart-carrier-rejected-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(path);
        using (var handler = new OfficeCli.Handlers.WordHandler(path, editable: true))
        {
            handler.Add("/body", "paragraph", null, new Dictionary<string, string>
            {
                ["text"] = "unchanged",
            });
            foreach (var (label, encoded, extraProperty) in cases)
            {
                var properties = new Dictionary<string, string>
                {
                    ["chartXmlBase64"] = encoded,
                    ["width"] = "1cm",
                    ["height"] = "1cm",
                };
                if (extraProperty is { } extra)
                    properties[extra.Key] = extra.Value;

                var rejected = false;
                try
                {
                    handler.Add("/body/p[1]", "chart", null, properties);
                }
                catch (ArgumentException)
                {
                    rejected = true;
                }
                Assert(rejected, $"unsafe raw chart case was accepted: {label}");
            }
            handler.Save();
        }
        using var rejectedDocument = WordprocessingDocument.Open(path, false);
        Assert(!rejectedDocument.MainDocumentPart!.ChartParts.Any(),
            "rejected raw chart payload left an orphan ChartPart behind");
    }
    finally
    {
        if (File.Exists(path)) File.Delete(path);
    }
}

static void TextboxOrdinalsIncludeNestedCellDrawings()
{
    var path = Path.Combine(Path.GetTempPath(), $"officecli-drawing-ordinals-{Guid.NewGuid():N}.docx");
    try
    {
        OfficeCli.BlankDocCreator.Create(path);
        using (var document = WordprocessingDocument.Open(path, true))
        {
            var body = document.MainDocumentPart!.Document!.Body!;
            body.RemoveAllChildren<Paragraph>();
            body.PrependChild(new Table(new TableRow(new TableCell(new Paragraph()))));
            body.AppendChild(new Paragraph());
            document.MainDocumentPart.Document.Save();
        }

        using (var handler = new OfficeCli.Handlers.WordHandler(path, editable: true))
        {
            var cellTextbox = handler.Add(
                "/body/tbl[1]/tr[1]/tc[1]/p[1]",
                "textbox",
                null,
                new Dictionary<string, string> { ["anchor"] = "false", ["text"] = "cell" });
            Assert(cellTextbox == "/body/tbl[1]/tr[1]/tc[1]/textbox[1]",
                $"unexpected cell textbox path: {cellTextbox}");

            var bodyTextbox = handler.Add(
                "/body/p[1]",
                "textbox",
                null,
                new Dictionary<string, string> { ["anchor"] = "false", ["text"] = "body" });
            Assert(bodyTextbox == "/body/textbox[2]",
                $"body textbox ordinal did not include the nested cell drawing: {bodyTextbox}");
            handler.Save();
        }
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

static void DumpReaderDirectNativeOutputIsNotWarned()
{
    var token = Guid.NewGuid().ToString("N");
    var extension = ".direct" + token;
    var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-" + token + extension);
    var sibling = Path.ChangeExtension(source, ".xlsx");
    var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
    var originalRegistration = Environment.GetEnvironmentVariable(registration);
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE");
    var originalError = Console.Error;
    DumpReaderInvoker.DumpResult? result = null;

    try
    {
        var sourceBytes = Encoding.UTF8.GetBytes("direct native output contract");
        File.WriteAllBytes(source, sourceBytes);
        Environment.SetEnvironmentVariable(registration, TestAppHostPath());
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
            "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
            "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", null);
        PluginRegistry.InvalidateCache();

        using var capturedError = new StringWriter();
        Console.SetError(capturedError);
        result = DumpReaderInvoker.Run(source, extension);
        Console.SetError(originalError);

        Assert(File.Exists(sibling), "direct-native plugin did not create its sibling");
        Assert(File.ReadAllBytes(sibling).SequenceEqual(sourceBytes),
            "host altered the plugin's direct-native sibling");
        Assert(!capturedError.ToString().Contains("will be blank", StringComparison.Ordinal),
            "host reported a direct-native output as a blank JSONL conversion");
    }
    finally
    {
        Console.SetError(originalError);
        Environment.SetEnvironmentVariable(registration, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", originalMode);
        PluginRegistry.InvalidateCache();
        if (result is not null) File.Delete(result.ConvertedPath);
        File.Delete(source);
        File.Delete(sibling);
    }
}

static void DumpReaderDirectNativeProtocolIsExclusive()
{
    foreach (var (mode, expectedCode) in new[]
    {
        ("whitespace", "plugin_contract_violation"),
        ("bom-only", "plugin_contract_violation"),
        ("json-and-sibling", "plugin_contract_violation"),
        ("no-sibling", "plugin_contract_violation"),
        ("fail-after-sibling", "corrupt_input"),
    })
    {
        var token = Guid.NewGuid().ToString("N");
        var extension = ".direct" + token;
        var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-contract-" + token + extension);
        var sibling = Path.ChangeExtension(source, ".xlsx");
        var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
        var originalRegistration = Environment.GetEnvironmentVariable(registration);
        var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
        var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
        var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE");
        try
        {
            File.WriteAllText(source, "direct native contract failure");
            Environment.SetEnvironmentVariable(registration, TestAppHostPath());
            Environment.SetEnvironmentVariable(
                "OFFICECLI_TEST_MANIFEST_JSON",
                "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
                "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
                "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
            Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
            Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", mode);
            PluginRegistry.InvalidateCache();

            CliException? failure = null;
            try { _ = DumpReaderInvoker.Run(source, extension); }
            catch (CliException ex) { failure = ex; }

            Assert(failure is not null, $"direct-native mode {mode} unexpectedly succeeded");
            Assert(failure!.Code == expectedCode,
                $"direct-native mode {mode} returned {failure.Code}, expected {expectedCode}");
            if (mode == "no-sibling")
                Assert(!File.Exists(sibling), "no-sibling mode unexpectedly created output");
        }
        finally
        {
            Environment.SetEnvironmentVariable(registration, originalRegistration);
            Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
            Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
            Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", originalMode);
            PluginRegistry.InvalidateCache();
            File.Delete(source);
            File.Delete(sibling);
        }
    }
}

static void DumpReaderDirectNativeFailurePreservesMatchingSibling()
{
    var token = Guid.NewGuid().ToString("N");
    var extension = ".direct" + token;
    var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-matching-race-" + token + extension);
    var sibling = Path.ChangeExtension(source, ".xlsx");
    var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
    var originalRegistration = Environment.GetEnvironmentVariable(registration);
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE");
    try
    {
        var sourceBytes = Encoding.UTF8.GetBytes("matching direct native source");
        File.WriteAllBytes(source, sourceBytes);
        Environment.SetEnvironmentVariable(registration, TestAppHostPath());
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
            "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
            "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", "fail-after-sibling");
        PluginRegistry.InvalidateCache();

        CliException? failure = null;
        try { _ = DumpReaderInvoker.Run(source, extension); }
        catch (CliException ex) { failure = ex; }

        Assert(failure?.Code == "corrupt_input",
            $"failed direct-native run returned {failure?.Code ?? "success"}");
        Assert(File.Exists(sibling), "host deleted a matching sibling path after plugin failure");
        Assert(File.ReadAllBytes(sibling).SequenceEqual(sourceBytes),
            "host altered the matching sibling after plugin failure");
    }
    finally
    {
        Environment.SetEnvironmentVariable(registration, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", originalMode);
        PluginRegistry.InvalidateCache();
        File.Delete(source);
        File.Delete(sibling);
    }
}

static void DumpReaderDirectNativeFailurePreservesUnownedSibling()
{
    var token = Guid.NewGuid().ToString("N");
    var extension = ".direct" + token;
    var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-race-" + token + extension);
    var sibling = Path.ChangeExtension(source, ".xlsx");
    var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
    var originalRegistration = Environment.GetEnvironmentVariable(registration);
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
    var originalMode = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE");
    var independentlyPublished = Encoding.UTF8.GetBytes("independently published native file");
    try
    {
        File.WriteAllText(source, "direct native source");
        Environment.SetEnvironmentVariable(registration, TestAppHostPath());
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
            "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
            "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", "foreign-sibling-on-failure");
        PluginRegistry.InvalidateCache();

        CliException? failure = null;
        try { _ = DumpReaderInvoker.Run(source, extension); }
        catch (CliException ex) { failure = ex; }

        Assert(failure?.Code == "corrupt_input",
            $"failed direct-native run returned {failure?.Code ?? "success"}");
        Assert(File.Exists(sibling), "host deleted a sibling it could not attribute to the failed plugin");
        Assert(File.ReadAllBytes(sibling).SequenceEqual(independentlyPublished),
            "host altered the unowned sibling after plugin failure");
    }
    finally
    {
        Environment.SetEnvironmentVariable(registration, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_MODE", originalMode);
        PluginRegistry.InvalidateCache();
        File.Delete(source);
        File.Delete(sibling);
    }
}

static void DumpReaderDirectNativePreexistingConflictIsPreserved()
{
    var token = Guid.NewGuid().ToString("N");
    var extension = ".direct" + token;
    var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-conflict-" + token + extension);
    var sibling = Path.ChangeExtension(source, ".xlsx");
    var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
    var originalRegistration = Environment.GetEnvironmentVariable(registration);
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
    try
    {
        File.WriteAllText(source, "foreign source");
        var existingBytes = Encoding.UTF8.GetBytes("independently published native file");
        File.WriteAllBytes(sibling, existingBytes);
        Environment.SetEnvironmentVariable(registration, TestAppHostPath());
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
            "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
            "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
        PluginRegistry.InvalidateCache();

        CliException? failure = null;
        try { _ = DumpReaderInvoker.Run(source, extension); }
        catch (CliException ex) { failure = ex; }

        Assert(failure?.Code == "plugin_output_conflict",
            $"preexisting sibling conflict returned {failure?.Code ?? "success"}");
        Assert(File.ReadAllBytes(sibling).SequenceEqual(existingBytes),
            "host launched the plugin and changed a different preexisting sibling");
    }
    finally
    {
        Environment.SetEnvironmentVariable(registration, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
        PluginRegistry.InvalidateCache();
        File.Delete(source);
        File.Delete(sibling);
    }
}

static void DumpReaderDirectNativePreexistingMutationIsRejected()
{
    var token = Guid.NewGuid().ToString("N");
    var extension = ".direct" + token;
    var source = Path.Combine(Path.GetTempPath(), "officecli-direct-native-existing-" + token + extension);
    var sibling = Path.ChangeExtension(source, ".xlsx");
    var registration = "OFFICECLI_PLUGIN_DUMP_READER_" + extension.TrimStart('.').ToUpperInvariant();
    var originalRegistration = Environment.GetEnvironmentVariable(registration);
    var originalManifest = Environment.GetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON");
    var originalTarget = Environment.GetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET");
    try
    {
        var bytes = Encoding.UTF8.GetBytes("identical direct native cache");
        File.WriteAllBytes(source, bytes);
        File.WriteAllBytes(sibling, bytes);
        File.SetLastWriteTimeUtc(source, DateTime.UtcNow.AddMinutes(-1));
        File.SetLastWriteTimeUtc(sibling, DateTime.UtcNow.AddHours(-1));
        Assert(File.GetLastWriteTimeUtc(source) != File.GetLastWriteTimeUtc(sibling),
            "test setup requires distinct source and sibling mtimes");
        Environment.SetEnvironmentVariable(registration, TestAppHostPath());
        Environment.SetEnvironmentVariable(
            "OFFICECLI_TEST_MANIFEST_JSON",
            "{\"name\":\"officecli-direct-native\",\"version\":\"1.0.0\",\"protocol\":1," +
            "\"kinds\":[\"dump-reader\"],\"extensions\":[\"" + extension + "\"],\"target\":\"xlsx\"," +
            "\"supports\":[\"direct-native\",\"byte-preserving\"]}");
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", ".xlsx");
        PluginRegistry.InvalidateCache();

        CliException? failure = null;
        try { _ = DumpReaderInvoker.Run(source, extension); }
        catch (CliException ex) { failure = ex; }

        Assert(failure?.Code == "plugin_contract_violation",
            $"preexisting sibling mutation returned {failure?.Code ?? "success"}");
    }
    finally
    {
        Environment.SetEnvironmentVariable(registration, originalRegistration);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_MANIFEST_JSON", originalManifest);
        Environment.SetEnvironmentVariable("OFFICECLI_TEST_DIRECT_NATIVE_TARGET", originalTarget);
        PluginRegistry.InvalidateCache();
        File.Delete(source);
        File.Delete(sibling);
    }
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

static void StyleSchemaAcceptsEmittedParagraphIndents()
{
    var schemaType = typeof(DumpReaderInvoker).Assembly.GetType("OfficeCli.Help.SchemaHelpLoader")
        ?? throw new TypeLoadException("OfficeCli.Help.SchemaHelpLoader");
    var method = schemaType.GetMethod(
        "ValidateProperties",
        BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(schemaType.FullName, "ValidateProperties");
    foreach (var (specificIndent, value) in new[]
    {
        ("firstLineIndent", "10pt"),
        ("hangingIndent", "10pt"),
    })
    {
        var props = new Dictionary<string, string>
        {
            ["id"] = "2",
            ["name"] = "개요 1",
            ["type"] = "paragraph",
            ["leftIndent"] = "20pt",
            [specificIndent] = value,
        };
        var unknown = method.Invoke(null, ["docx", "style", "add", props]) as IReadOnlyList<string>
            ?? throw new InvalidOperationException("ValidateProperties returned null");

        Assert(unknown.Count == 0,
            $"style schema rejected emitted paragraph indents: {string.Join(", ", unknown)}");
    }
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

static IDocumentHandler OpenContractFormatHandler(string filePath)
{
    var assembly = typeof(PluginRegistry).Assembly;
    var sessionType = assembly.GetType("OfficeCli.Core.Plugins.FormatHandlerSession")
        ?? throw new TypeLoadException("OfficeCli.Core.Plugins.FormatHandlerSession");
    var proxyType = assembly.GetType("OfficeCli.Core.Plugins.FormatHandlerProxy")
        ?? throw new TypeLoadException("OfficeCli.Core.Plugins.FormatHandlerProxy");
    var plugin = new ResolvedPlugin(TestAppHostPath(), new PluginManifest
    {
        Name = "officecli-format-wire",
        Version = "1.0.0",
        Protocol = 1,
        Kinds = ["format-handler"],
        Extensions = [".wire"],
        Runtime = "dotnet",
        IdleTimeoutSeconds = new PluginIdleTimeout { Default = 5 },
    });

    var session = Activator.CreateInstance(
        sessionType,
        BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic,
        binder: null,
        args: [filePath, plugin],
        culture: null)
        ?? throw new InvalidOperationException("could not create format-handler session");
    try
    {
        var start = sessionType.GetMethod("Start", BindingFlags.Instance | BindingFlags.Public)
            ?? throw new MissingMethodException(sessionType.FullName, "Start");
        start.Invoke(session, [true]);
        return Activator.CreateInstance(
            proxyType,
            BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic,
            binder: null,
            args: [session],
            culture: null) as IDocumentHandler
            ?? throw new InvalidOperationException("could not create format-handler proxy");
    }
    catch
    {
        (session as IDisposable)?.Dispose();
        throw;
    }
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
