/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

// Writes one JSON line per corpus case from the .NET packages.
//
//   dotnet run -- --corpus <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//   dotnet run -- fodid-sign ... and dotnet run -- fodid ..., see FodIdCases
//   dotnet run -- configuration ..., see ConfigurationCases
//   dotnet run -- derived ..., see DerivedCases
//   dotnet run -- robots-txt ..., see RobotsTxtCases
//   dotnet run -- metadata ..., see MetadataCases
//
// The pipelines and the line format are the same as the Rust producer's, as
// parity/README.md describes. Usage records go to a handler and to a custom
// usage sharing element inside this process, so nothing is sent anywhere.

using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using System.Xml;
using System.Xml.Linq;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.IpIntelligence.Cloud.FlowElements;
using FiftyOne.IpIntelligence.Engine.OnPremise.FlowElements;
using FiftyOne.IpIntelligence.Shared;
using FiftyOne.IpIntelligence.Translation.FlowElements;
using FiftyOne.Pipeline.AgentSignature.FlowElement;
using FiftyOne.Pipeline.CloudRequestEngine.FlowElements;
using FiftyOne.Pipeline.Core.Data;
using FiftyOne.Pipeline.Core.Data.Types;
using FiftyOne.Pipeline.Core.Exceptions;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.Engines;
using FiftyOne.Pipeline.Engines.Data;
using FiftyOne.Pipeline.Engines.FiftyOne.FlowElements;
using FiftyOne.Pipeline.JavaScriptBuilder.Data;
using FiftyOne.Pipeline.JavaScriptBuilder.FlowElement;
using FiftyOne.Pipeline.JsonBuilder.Data;
using FiftyOne.Pipeline.JsonBuilder.FlowElement;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    public static class Program
    {
        // The values read beside an element's listed properties, by the
        // element's data key. Device Detection's match metrics, and the
        // culture the countries translation engine sorted with, which it
        // writes without listing.
        private static readonly Dictionary<string, string[]> ExtraProperties =
            new Dictionary<string, string[]>
            {
                ["device"] = new[]
                {
                    "DeviceId", "Difference", "Drift", "Iterations",
                    "MatchedNodes", "Method", "UserAgents"
                },
                ["countrynamestranslated"] = new[] { "SortingCultureUsed" }
            };

        // The evidence the Sequence element reads and writes back.
        private const string SessionIdKey = "query.session-id";
        private const string SequenceKey = "query.sequence";

        // The pipeline of a case that runs through the values pipeline.
        private const string ValuesPipeline = "values";

        // The JavaScript builder's settings in the values pipeline.
        private const string ValuesEndpoint = "json";
        private const string ValuesObjectName = "fod";

        // The pipeline of a case that runs through the agent signature
        // pipeline.
        private const string AgentSignaturePipeline = "agent-signature";

        // The pipeline of a case that runs through the translation pipeline.
        private const string TranslationPipeline = "translation";

        // The pipeline of a case that runs through the cloud translation
        // pipeline.
        private const string CloudTranslationPipeline = "cloud-translation";

        // Long enough that the first fetch of a directory the handler serves
        // at once never runs out, and shorter than the slow agent's delay.
        private static readonly TimeSpan AgentSignatureWaitBudget =
            TimeSpan.FromSeconds(2);

        // Never contacted. The handler answers every request itself.
        private const string UsageUrl = "http://usage.invalid/new.ashx";

        // More entries than any corpus holds, so every record waits in the
        // queue and goes in one message when usage sharing is disposed. The
        // send loop drops an entry each time a message fills, which a smaller
        // message would set off.
        private const int UsageEntriesPerMessage = 10000;

        // A header that is not shared.
        private const string UsageBlockedHeader = "Referer";

        // A query string parameter that is shared, beside those whose names
        // start with 51D_.
        private const string UsageIncludedQueryParameter = "utm_source";

        // An evidence key and value that stop a request being shared, as a
        // health check's would.
        private const string UsageIgnoredPair = "header.x-health-check:true";

        public static int Main(string[] argv)
        {
            switch (argv.FirstOrDefault())
            {
                case FodIdCases.Sign: return FodIdCases.SignCases(argv[1..]);
                case FodIdCases.Read: return FodIdCases.ReadCases(argv[1..]);
                case ConfigurationCases.Run: return ConfigurationCases.RunCases(argv[1..]);
                case DerivedCases.Run: return DerivedCases.RunCases(argv[1..]);
                case RobotsTxtCases.Run: return RobotsTxtCases.RunCases(argv[1..]);
                case MetadataCases.Run: return MetadataCases.RunCases(argv[1..]);
            }
            var args = Args.Parse(argv);
            var cases = JsonNode.Parse(File.ReadAllText(args.Corpus)).AsArray();
            ILoggerFactory loggerFactory = NullLoggerFactory.Instance;
            var capture = new UsageCapture();
            using var http = new HttpClient(capture);

            var shareUsage = ConfigureUsage(
                new ShareUsageBuilder(loggerFactory, http)).Build();
            var customShareUsage = ConfigureUsage(
                new CustomShareUsageBuilder(loggerFactory)).Build();
            var device = new DeviceDetectionHashEngineBuilder(loggerFactory)
                .SetPerformanceProfile(PerformanceProfiles.HighPerformance)
                .SetAutoUpdate(false)
                .SetDataFileSystemWatcher(false)
                .SetDataUpdateOnStartup(false)
                .Build(args.DeviceDetection, false);
            var ip = new IpiOnPremiseEngineBuilder(loggerFactory)
                .SetPerformanceProfile(PerformanceProfiles.LowMemory)
                .SetAutoUpdate(false)
                .SetDataFileSystemWatcher(false)
                .SetDataUpdateOnStartup(false)
                .Build(args.IpIntelligence, false);
            var json = new JsonBuilderElementBuilder(loggerFactory).Build();
            var javascript = new JavaScriptBuilderElementBuilder(loggerFactory)
                .SetMinify(false)
                .Build();
            var pipeline = new PipelineBuilder(loggerFactory)
                .AddFlowElement(new SequenceElementBuilder(loggerFactory).Build())
                .AddFlowElement(shareUsage)
                .AddFlowElement(customShareUsage)
                .AddFlowElement(device)
                .AddFlowElement(ip)
                .AddFlowElement(json)
                .AddFlowElement(javascript)
                .SetSuppressProcessExceptions(true)
                .Build();
            // Sequence, the values element and the two builders, for the
            // cases that compare the JSON and the JavaScript byte for byte.
            var values = new PipelineBuilder(loggerFactory)
                .AddFlowElement(new SequenceElementBuilder(loggerFactory).Build())
                .AddFlowElement(new ValuesElement())
                .AddFlowElement(new JsonBuilderElementBuilder(loggerFactory).Build())
                .AddFlowElement(new JavaScriptBuilderElementBuilder(loggerFactory)
                    .SetEndpoint(ValuesEndpoint)
                    .SetObjectName(ValuesObjectName)
                    .SetEnableCookies(false)
                    .SetMinify(false)
                    .Build())
                .SetSuppressProcessExceptions(true)
                .Build();
            // Sequence and the agent signature element, whose key
            // directories come from a handler inside this program.
            using var directories = new HttpClient(new KeyDirectories());
            var agentSignature = new PipelineBuilder(loggerFactory)
                .AddFlowElement(new SequenceElementBuilder(loggerFactory).Build())
                .AddFlowElement(new AgentSignatureElementBuilder(loggerFactory)
                    .SetHttpClient(directories)
                    .SetWaitBudget(AgentSignatureWaitBudget)
                    .Build())
                .SetSuppressProcessExceptions(true)
                .Build();
            // Sequence, an element writing the country codes the case's
            // evidence gives, the two country translation engines and the
            // JSON builder.
            var translation = new PipelineBuilder(loggerFactory)
                .AddFlowElement(new SequenceElementBuilder(loggerFactory).Build())
                .AddFlowElement(new CountriesElement())
                .AddFlowElement(new CountryCodeTranslationEngineBuilder(
                    loggerFactory).Build())
                .AddFlowElement(new CountriesTranslationEngineBuilder(
                    loggerFactory).Build())
                .AddFlowElement(new JsonBuilderElementBuilder(loggerFactory).Build())
                .SetSuppressProcessExceptions(true)
                .Build();
            // Sequence, the cloud request engine on a stand-in cloud inside
            // this program, the cloud countries translation engine and the
            // JSON builder.
            using var cloud = new HttpClient(new StandInCloud());
            var cloudRequest = new CloudRequestEngineBuilder(loggerFactory, cloud)
                .SetEndPoint(StandInCloud.Endpoint)
                .SetResourceKey(StandInCloud.ResourceKey)
                .Build();
            var cloudTranslation = new PipelineBuilder(loggerFactory)
                .AddFlowElement(new SequenceElementBuilder(loggerFactory).Build())
                .AddFlowElement(cloudRequest)
                .AddFlowElement(new CloudCountriesTranslationEngineBuilder(
                    loggerFactory).Build())
                .AddFlowElement(new JsonBuilderElementBuilder(loggerFactory).Build())
                .SetSuppressProcessExceptions(true)
                .Build();
            // The builders' whole output is recorded instead, and the cloud
            // request engine's data is the case's own response.
            var skipped = new HashSet<string>
            {
                json.ElementDataKey, javascript.ElementDataKey,
                cloudRequest.ElementDataKey
            };

            var lines = new List<JsonObject>();
            // The lines whose case ran through usage sharing. A values case
            // may give a session id an engines' case also gives, and must
            // not be given that case's record.
            var sharedLines = new List<JsonObject>();
            foreach (var item in cases)
            {
                var chosen = item["pipeline"]?.GetValue<string>() switch
                {
                    null => pipeline,
                    ValuesPipeline => values,
                    AgentSignaturePipeline => agentSignature,
                    TranslationPipeline => translation,
                    CloudTranslationPipeline => cloudTranslation,
                    var other => throw new ArgumentException(
                        $"{item["id"]}: unknown pipeline {other}")
                };
                var line = RunCase(chosen, item.AsObject(), skipped);
                lines.Add(line);
                if (chosen == pipeline)
                {
                    sharedLines.Add(line);
                }
            }

            // Disposing usage sharing sends what is queued and waits for it.
            shareUsage.Dispose();
            customShareUsage.Dispose();
            if (customShareUsage.Failure != null)
            {
                throw new InvalidOperationException(
                    "The custom usage sharing element did not write its records",
                    customShareUsage.Failure);
            }
            var records = UsageRecords.BySession(capture.Messages());
            var customRecords = UsageRecords.BySession(
                customShareUsage.Documents());
            foreach (var line in sharedLines)
            {
                var sessionId = line["session"]?["id"]?.GetValue<string>();
                line["usage"] = UsageRecords.Find(records, sessionId);
                line["usageCustom"] = UsageRecords.Find(customRecords, sessionId);
            }

            using (var writer = new StreamWriter(args.Out, false,
                new UTF8Encoding(false)))
            {
                writer.NewLine = "\n";
                foreach (var line in lines)
                {
                    writer.WriteLine(line.ToJsonString());
                }
            }
            Console.Error.WriteLine(
                $"{lines.Count} cases written to {args.Out}, " +
                $"{records.Count} usage records and " +
                $"{customRecords.Count} custom usage records captured");
            cloudTranslation.Dispose();
            translation.Dispose();
            agentSignature.Dispose();
            values.Dispose();
            pipeline.Dispose();
            return 0;
        }

        private static JsonObject RunCase(
            IPipeline pipeline,
            JsonObject item,
            HashSet<string> skipped)
        {
            using var data = pipeline.CreateFlowData();
            foreach (var pair in item["evidence"].AsObject())
            {
                data.AddEvidence(pair.Key, pair.Value.GetValue<string>());
            }
            string processError = null;
            try
            {
                data.Process();
            }
            catch (Exception e)
            {
                processError = e.Message;
            }

            var evidence = data.GetEvidence().AsDictionary();
            var errors = new JsonArray();
            foreach (var error in data.Errors ?? Enumerable.Empty<IFlowError>())
            {
                errors.Add(new JsonObject
                {
                    ["element"] = error.FlowElement?.ElementDataKey,
                    ["message"] = error.ExceptionData?.Message
                });
            }
            return new JsonObject
            {
                ["id"] = item["id"].GetValue<string>(),
                ["processError"] = processError,
                ["properties"] = Properties(pipeline, data, skipped),
                ["session"] = new JsonObject
                {
                    ["id"] = Text(evidence, SessionIdKey),
                    ["sequence"] = Number(evidence, SequenceKey)
                },
                ["evidenceAfter"] = new JsonObject
                {
                    [SessionIdKey] = Text(evidence, SessionIdKey),
                    [SequenceKey] = Text(evidence, SequenceKey)
                },
                ["errors"] = errors,
                ["json"] = Try(() => data.Get<IJsonBuilderElementData>()?.Json),
                ["javascript"] = Try(() =>
                    data.Get<IJavaScriptBuilderElementData>()?.JavaScript),
                ["usage"] = null,
                ["usageCustom"] = null
            };
        }

        // Usage sharing's settings, the same as the Rust producer's, for the
        // element that posts and for the custom one.
        private static ShareUsageBuilderBase<T> ConfigureUsage<T>(
            ShareUsageBuilderBase<T> builder)
        {
            return builder
                .SetShareUsageUrl(UsageUrl)
                .SetSharePercentage(1)
                .SetMaximumQueueSize(UsageEntriesPerMessage)
                .SetMinimumEntriesPerMessage(UsageEntriesPerMessage)
                .SetRepeatEvidenceIntervalMinutes(0)
                .SetBlockedHttpHeader(UsageBlockedHeader)
                .SetIncludedQueryStringParameter(UsageIncludedQueryParameter)
                .SetIgnoreFlowDataEvidenceFilter(UsageIgnoredPair);
        }

        private static string Text(
            IReadOnlyDictionary<string, object> evidence,
            string key)
        {
            return evidence.TryGetValue(key, out var value)
                ? value?.ToString() : null;
        }

        private static JsonNode Number(
            IReadOnlyDictionary<string, object> evidence,
            string key)
        {
            return evidence.TryGetValue(key, out var value) &&
                long.TryParse(value?.ToString(), out var number)
                ? JsonValue.Create(number) : null;
        }

        private static string Try(Func<string> read)
        {
            try
            {
                return read();
            }
            catch (Exception)
            {
                return null;
            }
        }

        // Every property of every element that lists properties, keyed by
        // the element's data key and the lowercase property name. The two
        // builders are left out because their whole output is recorded.
        private static JsonObject Properties(
            IPipeline pipeline,
            IFlowData data,
            HashSet<string> skipped)
        {
            var all = data.ElementDataAsDictionary();
            var elements = new JsonObject();
            foreach (var element in pipeline.FlowElements)
            {
                var key = element.ElementDataKey;
                if (skipped.Contains(key))
                {
                    continue;
                }
                var names = element.Properties.Select(p => p.Name).ToList();
                if (names.Count == 0)
                {
                    continue;
                }
                if (ExtraProperties.TryGetValue(key, out var extra))
                {
                    names.AddRange(extra.Where(m => names.Any(n =>
                        string.Equals(n, m, StringComparison.OrdinalIgnoreCase))
                        == false));
                }
                if (all.TryGetValue(key, out var found) == false ||
                    found is IElementData elementData == false)
                {
                    elements[key] = null;
                    continue;
                }
                var values = new JsonObject();
                foreach (var name in names)
                {
                    values[name.ToLowerInvariant()] = Read(elementData, name);
                }
                elements[key] = values;
            }
            return elements;
        }

        internal static JsonObject Read(IElementData data, string name)
        {
            object value;
            try
            {
                value = data[name];
            }
            catch (Exception e)
            {
                return new JsonObject { ["name"] = name, ["missing"] = e.Message };
            }
            if (value is IAspectPropertyValue aspect)
            {
                if (aspect.HasValue == false)
                {
                    return new JsonObject
                    {
                        ["name"] = name,
                        ["noValue"] = aspect.NoValueMessage
                    };
                }
                value = aspect.Value;
            }
            var (type, node) = Typed(value);
            return new JsonObject
            {
                ["name"] = name,
                ["type"] = type,
                ["value"] = node
            };
        }

        private static (string, JsonNode) Typed(object value)
        {
            switch (value)
            {
                case null: return ("null", null);
                case string s: return ("string", s);
                case bool b: return ("bool", b);
                case int i: return ("integer", i);
                case long l: return ("integer", l);
                case short s: return ("integer", s);
                case uint u: return ("integer", u);
                case ushort u: return ("integer", u);
                case byte b: return ("integer", b);
                case float f: return ("double", (double)f);
                case double d: return ("double", d);
                case JavaScript js: return ("javascript", js.ToString());
                // RFC 3339 with the offset written out, the form .NET's JSON
                // builder writes.
                case DateTimeOffset time:
                    return ("dateTime", time.ToString(
                        "yyyy-MM-dd'T'HH:mm:sszzz",
                        CultureInfo.InvariantCulture));
                case IPAddress address: return ("ipAddress", address.ToString());
                case IEnumerable<string> list:
                    return ("stringList", new JsonArray(
                        list.Select(s => (JsonNode)s).ToArray()));
                case IEnumerable list:
                    var items = list.Cast<object>().ToList();
                    if (items.Count > 0 && items.All(IsWeighted))
                    {
                        return ("weightedList", new JsonArray(
                            items.Select(Weighted).ToArray()));
                    }
                    return ("list", new JsonArray(items
                        .Select(item => (JsonNode)new JsonObject
                        {
                            ["type"] = Typed(item).Item1,
                            ["value"] = Typed(item).Item2
                        }).ToArray()));
                // The type the crates and the metadata lines name a WKT value.
                case object wkt when wkt.GetType().Name == "WktString":
                    return ("wktString", wkt.ToString());
                default:
                    return ("other:" + value.GetType().Name, value.ToString());
            }
        }

        // IWeightedValue<T> is generic, so its members are read by name.
        private static bool IsWeighted(object item)
        {
            var type = item?.GetType();
            return type?.GetProperty("RawWeighting") != null &&
                type.GetProperty("Value") != null;
        }

        private static JsonNode Weighted(object item)
        {
            var type = item.GetType();
            return new JsonObject
            {
                ["rawWeighting"] = JsonValue.Create(Convert.ToInt64(
                    type.GetProperty("RawWeighting").GetValue(item))),
                ["value"] = Typed(type.GetProperty("Value").GetValue(item)).Item2
            };
        }

        private sealed class Args
        {
            public string Corpus;
            public string DeviceDetection;
            public string IpIntelligence;
            public string Out;

            public static Args Parse(string[] argv)
            {
                var args = new Args();
                for (var i = 0; i + 1 < argv.Length; i += 2)
                {
                    switch (argv[i])
                    {
                        case "--corpus": args.Corpus = argv[i + 1]; break;
                        case "--dd": args.DeviceDetection = argv[i + 1]; break;
                        case "--ipi": args.IpIntelligence = argv[i + 1]; break;
                        case "--out": args.Out = argv[i + 1]; break;
                        default:
                            throw new ArgumentException(
                                $"unknown argument {argv[i]}");
                    }
                }
                if (args.Corpus == null || args.DeviceDetection == null ||
                    args.IpIntelligence == null || args.Out == null)
                {
                    throw new ArgumentException(
                        "--corpus, --dd, --ipi and --out are required");
                }
                return args;
            }
        }
    }

    // The values element, whose data holds one of each kind of value the JSON
    // builder writes and which adds two errors and a warning, so the JSON and
    // the JavaScript the two languages write for the same flow data can be
    // compared byte for byte. The Rust producer's values element holds the
    // same values in the same order. Only the properties something reads by
    // their metadata are declared, being a delayed JavaScript property and
    // one that names it as evidence, which the JSON builder reads, and
    // Promise, which the JavaScript builder reads. The JSON builder writes
    // the rest from the data alone.
    internal sealed class ValuesElement :
        FlowElementBase<ValuesData, IElementPropertyMetaData>
    {
        private const string Key = "values";

        // The errors the element adds, in order.
        private static readonly string[] ErrorMessages =
        {
            "The first error.", "The second error."
        };

        // The warning the element adds after its errors.
        private const string WarningMessage = "The warning.";

        // The reason the property with no value gives.
        private const string NoValueReason = "There is no value, on purpose.";

        // The evidence that gives the Promise property its value. Without
        // it, Promise has no value.
        private const string PromiseEvidenceKey = "header.values-promise";

        // The reason Promise gives when the evidence has no value for it.
        private const string NoPromiseReason = "The evidence gave no promise.";

        private readonly IList<IElementPropertyMetaData> _properties;

        private readonly IEvidenceKeyFilter _filter =
            new EvidenceKeyFilterWhitelist(
                new List<string> { PromiseEvidenceKey });

        public ValuesElement() : base(
            NullLogger<FlowElementBase<ValuesData, IElementPropertyMetaData>>
                .Instance)
        {
            _properties = new List<IElementPropertyMetaData>
            {
                new ElementPropertyMetaData(
                    this, "Script", typeof(JavaScript), true,
                    delayExecution: true),
                new ElementPropertyMetaData(
                    this, "FromScript", typeof(string), true,
                    evidenceProperties: new[] { "Script" }),
                new ElementPropertyMetaData(
                    this, "Promise", typeof(string), true)
            };
        }

        public override string ElementDataKey => Key;

        public override IEvidenceKeyFilter EvidenceKeyFilter => _filter;

        public override IList<IElementPropertyMetaData> Properties =>
            _properties;

        protected override void ProcessInternal(IFlowData data)
        {
            var values = data.GetOrAdd(
                ElementDataKeyTyped,
                pipeline => new ValuesData(pipeline));
            values["Text"] = "a \"quote\", a backslash \\, U+0001 \u0001, " +
                "U+2028 \u2028, <, & and \u00e9";
            values["Whitespace"] = "a tab \t, a new line \n, a return \r, " +
                "U+0085 \u0085 and U+2029 \u2029";
            values["One"] = 1.0;
            values["Tenth"] = 0.1;
            values["Small"] = 1e-7;
            values["Large"] = 1e21;
            values["Single"] = 0.1f;
            values["SingleLarge"] = 1e9f;
            values["Count"] = 42;
            values["Flag"] = true;
            values["Names"] = new List<string> { "first", "second" };
            values["Missing"] = new AspectPropertyValue<string>
            {
                NoValueMessage = NoValueReason
            };
            values["Script"] = new JavaScript(
                "document.cookie = \"51D_Values=1\";");
            values["FromScript"] = "Read from the script.";
            values["Weights"] = new List<IWeightedValue<string>>
            {
                new WeightedValue<string>(40000, "first"),
                new WeightedValue<string>(25535, "second")
            };
            // With no evidence for it Promise has no value, as Device
            // Detection's Promise has none when it has no answer.
            values["Promise"] = data.TryGetEvidence(
                PromiseEvidenceKey, out string promise)
                ? new AspectPropertyValue<string>(promise)
                : new AspectPropertyValue<string>
                {
                    NoValueMessage = NoPromiseReason
                };
            foreach (var message in ErrorMessages)
            {
                data.AddError(new PipelineException(message), this);
            }
            data.AddWarning(WarningMessage, this);
        }

        protected override void ManagedResourcesCleanup() { }

        protected override void UnmanagedResourcesCleanup() { }
    }

    // The values element's data, in the order the properties were set.
    internal sealed class ValuesData : ElementDataBase
    {
        public ValuesData(IPipeline pipeline)
            : base(NullLogger<ElementDataBase>.Instance, pipeline)
        {
        }
    }

    // Writes the IP Intelligence country codes the case's evidence gives,
    // under IP Intelligence's data key, and nothing when the evidence gives
    // none. Each property's evidence is code:raw weighting items separated
    // by commas. A property whose evidence is absent is not set, and one
    // whose evidence is empty has no value. The codes come from the evidence
    // rather than a data file, so both languages translate the same codes
    // and weightings. It lists no properties, so the line leaves it out and
    // only the JSON shows its data. The Rust producer's element writes the
    // same codes as the crates' IP Intelligence data.
    internal sealed class CountriesElement :
        FlowElementBase<CountriesData, IElementPropertyMetaData>
    {
        private const string Key = "ip";

        // Each property and the evidence holding its codes.
        private static readonly (string Name, string Evidence)[] Codes =
        {
            ("CountryCodesGeographical", "query.parity-geographical"),
            ("CountryCodesPopulation", "query.parity-population")
        };

        // The reason a property with empty evidence gives.
        private const string NoCodesReason =
            "The evidence gave no country codes.";

        private readonly IEvidenceKeyFilter _filter =
            new EvidenceKeyFilterWhitelist(
                Codes.Select(c => c.Evidence).ToList());

        private readonly IList<IElementPropertyMetaData> _properties =
            new List<IElementPropertyMetaData>();

        public CountriesElement() : base(
            NullLogger<FlowElementBase<CountriesData, IElementPropertyMetaData>>
                .Instance)
        {
        }

        public override string ElementDataKey => Key;

        public override IEvidenceKeyFilter EvidenceKeyFilter => _filter;

        public override IList<IElementPropertyMetaData> Properties =>
            _properties;

        protected override void ProcessInternal(IFlowData data)
        {
            var entries = new List<(string, object)>();
            foreach (var (name, evidence) in Codes)
            {
                if (data.TryGetEvidence(evidence, out string text))
                {
                    entries.Add((name, WeightedCodes(text)));
                }
            }
            if (entries.Count == 0)
            {
                return;
            }
            var countries = data.GetOrAdd(
                ElementDataKeyTyped,
                pipeline => new CountriesData(pipeline));
            foreach (var (name, value) in entries)
            {
                countries[name] = value;
            }
        }

        private static IAspectPropertyValue<IReadOnlyList<IWeightedValue<string>>>
            WeightedCodes(string text)
        {
            if (text.Length == 0)
            {
                return new AspectPropertyValue<
                    IReadOnlyList<IWeightedValue<string>>>
                {
                    NoValueMessage = NoCodesReason
                };
            }
            var codes = new List<IWeightedValue<string>>();
            foreach (var item in text.Split(','))
            {
                var colon = item.IndexOf(':');
                if (colon < 0)
                {
                    throw new ArgumentException(
                        $"{item} is not code:raw weighting");
                }
                codes.Add(new WeightedValue<string>(
                    ushort.Parse(item.Substring(colon + 1),
                        NumberStyles.None, CultureInfo.InvariantCulture),
                    item.Substring(0, colon)));
            }
            return new AspectPropertyValue<
                IReadOnlyList<IWeightedValue<string>>>(codes);
        }

        protected override void ManagedResourcesCleanup() { }

        protected override void UnmanagedResourcesCleanup() { }
    }

    // The country codes, as IP Intelligence data, so the countries
    // translation engine finds them as it finds an IP Intelligence engine's.
    // No engine made them, so a property that was not set reads as null.
    internal sealed class CountriesData : IpIntelligenceData
    {
        public CountriesData(IPipeline pipeline)
            : base(NullLogger<AspectDataBase>.Instance, pipeline, null, null)
        {
        }
    }

    // Serves the agents' key directories in place of the network. The slow
    // agent answers after the wait budget, so its first request reports
    // Timeout. Any other address is not found.
    internal sealed class KeyDirectories : HttpMessageHandler
    {
        private const string Agent =
            "https://agent.parity.test/.well-known/http-message-signatures-directory";

        private const string SlowAgent =
            "https://slow-agent.parity.test/.well-known/http-message-signatures-directory";

        private const string MediaType =
            "application/http-message-signatures-directory+json";

        // The public part of the Ed25519 test key of RFC 9421 Appendix
        // B.1.4.
        private const string DirectoryJson =
            "{\"keys\":[{\"kty\":\"OKP\",\"crv\":\"Ed25519\"," +
            "\"kid\":\"test-key-ed25519\"," +
            "\"x\":\"JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs\"}]," +
            "\"purpose\":\"parity\"}";

        private static readonly TimeSpan SlowDelay = TimeSpan.FromSeconds(10);

        protected override async Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken)
        {
            var url = request.RequestUri?.AbsoluteUri;
            if (url == SlowAgent)
            {
                await Task.Delay(SlowDelay, cancellationToken);
            }
            else if (url != Agent)
            {
                return new HttpResponseMessage(HttpStatusCode.NotFound)
                {
                    RequestMessage = request
                };
            }
            return new HttpResponseMessage(HttpStatusCode.OK)
            {
                RequestMessage = request,
                Content = new StringContent(DirectoryJson, Encoding.UTF8, MediaType)
            };
        }
    }

    // The cloud of the cloud translation pipeline, the same as the Rust
    // producer's. It grants the six translated properties, takes the two
    // parity evidence keys, and answers each data request with the body and
    // status those keys carry, so each case gives both languages the same
    // cloud response.
    internal sealed class StandInCloud : HttpMessageHandler
    {
        // Never contacted. The handler answers every request itself.
        public const string Endpoint = "https://cloud.parity.invalid/api/v4/";

        // Not a real key. The handler grants it the six properties.
        public const string ResourceKey = "parity-resource-key";

        // The form fields that carry the body and the status of the
        // response, as the evidence keys query.parity-cloud-response and
        // query.parity-cloud-status reach the cloud. With no status the
        // response is 200, and with no body it is empty.
        private const string ResponseField = "parity-cloud-response";
        private const string StatusField = "parity-cloud-status";

        // The six properties .NET's engine tests list, with the types the
        // cloud gives them. The same text as the Rust producer's.
        private const string AccessibleProperties =
            "{\"Products\":{\"countrynamestranslated\":{\"DataTier\":\"CloudV4\"," +
            "\"Properties\":[" +
            "{\"Name\":\"CountryNamesGeographicalTranslated\",\"Type\":\"WeightedString\"}," +
            "{\"Name\":\"CountryNamesPopulationTranslated\",\"Type\":\"WeightedString\"}," +
            "{\"Name\":\"CountryNamesGeographicalAllTranslated\",\"Type\":\"Array\"}," +
            "{\"Name\":\"CountryNamesPopulationAllTranslated\",\"Type\":\"Array\"}," +
            "{\"Name\":\"CountryCodesGeographicalAll\",\"Type\":\"Array\"}," +
            "{\"Name\":\"CountryCodesPopulationAll\",\"Type\":\"Array\"}]}}}";

        // The evidence the handler takes. The same text as the Rust
        // producer's.
        private const string EvidenceKeys =
            "[\"query.parity-cloud-response\",\"query.parity-cloud-status\"]";

        protected override async Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken)
        {
            var url = request.RequestUri?.AbsoluteUri ?? string.Empty;
            var status = HttpStatusCode.OK;
            string body;
            if (url.Contains("accessibleproperties"))
            {
                body = AccessibleProperties;
            }
            else if (url.Contains("evidencekeys"))
            {
                body = EvidenceKeys;
            }
            else
            {
                var form = await Form(request.Content);
                if (form.TryGetValue(StatusField, out var text))
                {
                    status = (HttpStatusCode)int.Parse(
                        text, CultureInfo.InvariantCulture);
                }
                body = form.TryGetValue(ResponseField, out var response)
                    ? response : string.Empty;
            }
            return new HttpResponseMessage(status)
            {
                RequestMessage = request,
                Content = new StringContent(body, Encoding.UTF8, "application/json")
            };
        }

        // The fields of a URL-encoded form, decoded.
        private static async Task<Dictionary<string, string>> Form(
            HttpContent content)
        {
            var form = new Dictionary<string, string>();
            if (content == null)
            {
                return form;
            }
            var text = await content.ReadAsStringAsync();
            foreach (var pair in text.Split('&', StringSplitOptions.RemoveEmptyEntries))
            {
                var at = pair.IndexOf('=');
                var name = at < 0 ? pair : pair.Substring(0, at);
                var value = at < 0 ? string.Empty : pair.Substring(at + 1);
                form[WebUtility.UrlDecode(name)] = WebUtility.UrlDecode(value);
            }
            return form;
        }
    }

    // Takes the usage sharing messages in place of the network and answers
    // each one as the service would.
    internal sealed class UsageCapture : HttpMessageHandler
    {
        private readonly List<string> _messages = new List<string>();

        protected override async Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken)
        {
            var bytes = await request.Content.ReadAsByteArrayAsync(
                cancellationToken);
            if (request.Content.Headers.ContentEncoding.Contains("gzip"))
            {
                using var input = new GZipStream(
                    new MemoryStream(bytes), CompressionMode.Decompress);
                using var output = new MemoryStream();
                await input.CopyToAsync(output, cancellationToken);
                bytes = output.ToArray();
            }
            lock (_messages)
            {
                _messages.Add(Encoding.UTF8.GetString(bytes));
            }
            return new HttpResponseMessage(HttpStatusCode.OK);
        }

        // A copy of the messages taken so far.
        public List<string> Messages()
        {
            lock (_messages)
            {
                return new List<string>(_messages);
            }
        }
    }

    internal static class UsageRecords
    {
        // Each record's text exactly as .NET wrote it, by its session id.
        // Records are cut from the message text rather than written again
        // from a parsed document, which would change how values are escaped.
        public static Dictionary<string, string> BySession(
            IEnumerable<string> messages)
        {
            const string open = "<Device>";
            const string close = "</Device>";
            var records = new Dictionary<string, string>();
            foreach (var message in messages)
            {
                var start = message.IndexOf(open, StringComparison.Ordinal);
                while (start >= 0)
                {
                    var end = message.IndexOf(close, start,
                        StringComparison.Ordinal);
                    if (end < 0)
                    {
                        break;
                    }
                    var record = message.Substring(
                        start, end + close.Length - start);
                    var sessionId = XElement.Parse(record)
                        .Element("SessionId")?.Value;
                    if (sessionId != null)
                    {
                        records[sessionId] = record;
                    }
                    start = message.IndexOf(open, end,
                        StringComparison.Ordinal);
                }
            }
            return records;
        }

        // The record written with the session id, or null.
        public static string Find(
            Dictionary<string, string> records,
            string sessionId)
        {
            return sessionId != null &&
                records.TryGetValue(sessionId, out var record)
                ? record : null;
        }
    }

    // Builds the custom usage sharing element from the same settings as
    // ShareUsageBuilder gives ShareUsageElement.
    internal sealed class CustomShareUsageBuilder :
        ShareUsageBuilderBase<CustomShareUsage>
    {
        public CustomShareUsageBuilder(ILoggerFactory loggerFactory)
            : base(loggerFactory)
        {
        }

        public override CustomShareUsage Build()
        {
            return new CustomShareUsage(
                LoggerFactory.CreateLogger<CustomShareUsage>(),
                SharePercentage,
                MinimumEntriesPerMessage,
                MaximumQueueSize,
                AddTimeout,
                TakeTimeout,
                RepeatEvidenceInterval,
                TrackSession,
                ShareUsageUri.AbsoluteUri,
                BlockedHttpHeaders,
                IncludedQueryStringParameters,
                IgnoreDataEvidenceFilter,
                AspSessionCookieName,
                ShareAllEvidence);
        }
    }

    // A usage sharing element that keeps each message in memory. It writes
    // an element before and after each record through WriteData, and its own
    // product and Flow Element names through WritePipelineInfo. The Rust
    // producer's custom sink writes the same.
    internal sealed class CustomShareUsage : ShareUsageBase
    {
        private const string CustomDataKey = "customshareusage";

        private const string BeforeName = "Before";
        private const string BeforeText = "a & <b> \"c\" é中";
        private const string AfterName = "After";
        private const string AfterPrefix = "Sequence ";

        private const string Product = "Parity";
        private static readonly string[] FlowElementNames =
        {
            "Custom element 1", "Custom element 2"
        };

        private readonly List<string> _documents = new List<string>();

        public CustomShareUsage(
            ILogger<ShareUsageBase> logger,
            double sharePercentage,
            int minimumEntriesPerMessage,
            int maximumQueueSize,
            int addTimeout,
            int takeTimeout,
            int repeatEvidenceIntervalMinutes,
            bool trackSession,
            string shareUsageUrl,
            List<string> blockedHttpHeaders,
            List<string> includedQueryStringParameters,
            List<KeyValuePair<string, string>> ignoreDataEvidenceFilter,
            string aspSessionCookieName,
            bool shareAllEvidence)
            : base(
                logger,
                null,
                sharePercentage,
                minimumEntriesPerMessage,
                maximumQueueSize,
                addTimeout,
                takeTimeout,
                repeatEvidenceIntervalMinutes,
                trackSession,
                shareUsageUrl,
                blockedHttpHeaders,
                includedQueryStringParameters,
                ignoreDataEvidenceFilter,
                aspSessionCookieName,
                null,
                shareAllEvidence)
        {
        }

        public override string ElementDataKey => CustomDataKey;

        // What stopped a message being written, or null. The base class
        // logs such a failure, and this program's logger writes nothing.
        public Exception Failure { get; private set; }

        // A copy of the documents written so far.
        public List<string> Documents()
        {
            lock (_documents)
            {
                return new List<string>(_documents);
            }
        }

        // Writes the queued records into one document in memory, as
        // ShareUsageElement writes them before compressing and posting.
        protected override Task BuildAndSendXmlAsync()
        {
            try
            {
                var queued = new List<ShareUsageData>();
                while (queued.Count < MinEntriesPerMessage * 2 &&
                    EvidenceCollection.TryTake(out var taken, TakeTimeout))
                {
                    queued.Add(taken);
                }
                if (queued.Count > 0)
                {
                    using var memory = new MemoryStream();
                    using (var writer = XmlWriter.Create(memory, WriterSettings))
                    {
                        writer.WriteStartElement("Devices");
                        foreach (var record in queued)
                        {
                            WriteData(writer, record);
                        }
                        writer.WriteEndElement();
                    }
                    lock (_documents)
                    {
                        _documents.Add(Encoding.UTF8.GetString(memory.ToArray()));
                    }
                }
            }
            catch (Exception e)
            {
                Failure = e;
            }
            return Task.CompletedTask;
        }

        protected override void WriteData(XmlWriter writer, ShareUsageData data)
        {
            writer.WriteStartElement("Device");
            writer.WriteElementString(BeforeName, BeforeText);
            WriteDeviceData(writer, data);
            writer.WriteElementString(AfterName, AfterPrefix +
                data.Sequence.ToString(CultureInfo.InvariantCulture));
            writer.WriteEndElement();
        }

        protected override void WritePipelineInfo(XmlWriter writer)
        {
            writer.WriteElementString("Product", Product);
            foreach (var name in FlowElementNames)
            {
                writer.WriteElementString("FlowElement", name);
            }
        }
    }
}
