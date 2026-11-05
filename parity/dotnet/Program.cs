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
//
// The pipelines and the line format are the same as the Rust producer's, as
// parity/README.md describes. Usage records go to a handler inside this
// process, so nothing is sent anywhere.

using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using System.Xml.Linq;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.IpIntelligence.Engine.OnPremise.FlowElements;
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
        // The element whose data also gets the match metrics read.
        private const string DeviceDataKey = "device";

        // Match metrics, read whether or not the engine lists them as
        // properties.
        private static readonly string[] MatchMetrics =
        {
            "DeviceId", "Difference", "Drift", "Iterations", "MatchedNodes",
            "Method", "UserAgents"
        };

        // The evidence the Sequence element reads and writes back.
        private const string SessionIdKey = "query.session-id";
        private const string SequenceKey = "query.sequence";

        // The pipeline of a case that runs through the values pipeline.
        private const string ValuesPipeline = "values";

        // The JavaScript builder's settings in the values pipeline.
        private const string ValuesEndpoint = "json";
        private const string ValuesObjectName = "fod";

        // Never contacted. The handler answers every request itself.
        private const string UsageUrl = "http://usage.invalid/new.ashx";

        // More entries than any corpus holds, so every record waits in the
        // queue and goes in one message when usage sharing is disposed. The
        // send loop drops an entry each time a message fills, which a smaller
        // message would set off.
        private const int UsageEntriesPerMessage = 10000;

        public static int Main(string[] argv)
        {
            var args = Args.Parse(argv);
            var cases = JsonNode.Parse(File.ReadAllText(args.Corpus)).AsArray();
            ILoggerFactory loggerFactory = NullLoggerFactory.Instance;
            var capture = new UsageCapture();
            using var http = new HttpClient(capture);

            var shareUsage = new ShareUsageBuilder(loggerFactory, http)
                .SetShareUsageUrl(UsageUrl)
                .SetSharePercentage(1)
                .SetMaximumQueueSize(UsageEntriesPerMessage)
                .SetMinimumEntriesPerMessage(UsageEntriesPerMessage)
                .SetRepeatEvidenceIntervalMinutes(0)
                .Build();
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
            var skipped = new HashSet<string>
            {
                json.ElementDataKey, javascript.ElementDataKey
            };

            var lines = new List<JsonObject>();
            foreach (var item in cases)
            {
                var chosen = item["pipeline"]?.GetValue<string>() switch
                {
                    null => pipeline,
                    ValuesPipeline => values,
                    var other => throw new ArgumentException(
                        $"{item["id"]}: unknown pipeline {other}")
                };
                lines.Add(RunCase(chosen, item.AsObject(), skipped));
            }

            // Disposing usage sharing sends what is queued and waits for it.
            shareUsage.Dispose();
            var records = capture.RecordsBySession();
            foreach (var line in lines)
            {
                var sessionId = line["session"]?["id"]?.GetValue<string>();
                line["usage"] = sessionId != null &&
                    records.TryGetValue(sessionId, out var record)
                    ? record : null;
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
                $"{records.Count} usage records captured");
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
                ["usage"] = null
            };
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
                if (key == DeviceDataKey)
                {
                    names.AddRange(MatchMetrics.Where(m => names.Any(n =>
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

        private static JsonObject Read(IElementData data, string name)
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

        // Each record's text exactly as .NET wrote it, by its session id.
        // Records are cut from the message text rather than written again
        // from a parsed document, which would change how values are escaped.
        public Dictionary<string, string> RecordsBySession()
        {
            const string open = "<Device>";
            const string close = "</Device>";
            var records = new Dictionary<string, string>();
            lock (_messages)
            {
                foreach (var message in _messages)
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
            }
            return records;
        }
    }
}
