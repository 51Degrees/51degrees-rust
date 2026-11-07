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

// The configuration cases: each builds a pipeline from a configuration file
// with FiftyOnePipelineBuilder, as .NET's examples do, and writes what was
// built, or the error.
//
//   dotnet run -- configuration --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//
// parity/README.md ("The configuration comparison") describes the cases and
// the line. Nothing is processed, and the usage sharing element's HttpClient
// refuses to send, so nothing is sent anywhere.

using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Reflection;
using System.Text;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.DeviceDetection.RobotsTxt.FlowElements;
using FiftyOne.IpIntelligence.Engine.OnPremise.FlowElements;
using FiftyOne.Pipeline.Core.Configuration;
using FiftyOne.Pipeline.Core.Exceptions;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.Engines.Data;
using FiftyOne.Pipeline.Engines.FiftyOne.FlowElements;
using FiftyOne.Pipeline.Engines.FlowElements;
using FiftyOne.Pipeline.JsonBuilder.FlowElement;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    internal static class ConfigurationCases
    {
        // The first argument that selects this form.
        public const string Run = "configuration";

        // The text in a case's configuration that stands for each data file.
        private const string DdPlaceholder = "{dd}";
        private const string IpiPlaceholder = "{ipi}";

        // The evidence keys given to the usage sharing element's filter,
        // whose answers show the blocked headers, the query string
        // parameters shared and sharing all evidence as the element applies
        // them, because .NET holds those settings only in that filter.
        private static readonly string[] UsageFilterKeys =
        {
            "header.user-agent", "header.referer", "header.x-forwarded-for",
            "header.accept-language", "query.utm_source", "query.utm_medium",
            "query.utm_campaign", "cookie.other"
        };

        private sealed class NoSend : HttpMessageHandler
        {
            protected override Task<HttpResponseMessage> SendAsync(
                HttpRequestMessage request, CancellationToken token) =>
                throw new InvalidOperationException(
                    "The configuration cases send nothing.");
        }

        public static int RunCases(string[] argv)
        {
            string cases = null, dd = null, ipi = null, outPath = null;
            for (var i = 0; i + 1 < argv.Length; i += 2)
            {
                switch (argv[i])
                {
                    case "--cases": cases = argv[i + 1]; break;
                    case "--dd": dd = Path.GetFullPath(argv[i + 1]); break;
                    case "--ipi": ipi = Path.GetFullPath(argv[i + 1]); break;
                    case "--out": outPath = argv[i + 1]; break;
                    default:
                        throw new ArgumentException(
                            $"unknown argument {argv[i]}");
                }
            }
            if (cases == null || dd == null || ipi == null || outPath == null)
            {
                throw new ArgumentException(
                    "--cases, --dd, --ipi and --out are required");
            }
            var files = new Dictionary<string, string>
            {
                [DdPlaceholder] = dd,
                [IpiPlaceholder] = ipi
            };
            using var writer = new StreamWriter(outPath, false,
                new UTF8Encoding(false));
            writer.NewLine = "\n";
            foreach (var node in JsonNode.Parse(File.ReadAllText(cases)).AsArray())
            {
                var line = new JsonObject
                {
                    ["id"] = node["id"].GetValue<string>(),
                    ["configuration"] = DescribeCase(
                        WithFiles(node["configuration"], files), files)
                };
                writer.WriteLine(line.ToJsonString());
            }
            return 0;
        }

        // The configuration with each string that is a placeholder replaced
        // by its data file's path.
        private static JsonNode WithFiles(
            JsonNode node, IReadOnlyDictionary<string, string> files)
        {
            switch (node)
            {
                case JsonObject o:
                    var copy = new JsonObject();
                    foreach (var (key, value) in o)
                    {
                        copy[key] = WithFiles(value, files);
                    }
                    return copy;
                case JsonArray a:
                    return new JsonArray(a.Select(v => WithFiles(v, files)).ToArray());
                case JsonValue v when v.TryGetValue<string>(out var text)
                    && files.TryGetValue(text, out var path):
                    return JsonValue.Create(path);
                default:
                    return node?.DeepClone();
            }
        }

        private static JsonObject DescribeCase(
            JsonNode configuration, IReadOnlyDictionary<string, string> files)
        {
            var options = new PipelineOptions();
            new ConfigurationBuilder()
                .AddJsonStream(new MemoryStream(Encoding.UTF8.GetBytes(
                    configuration.ToJsonString())))
                .Build()
                .Bind("PipelineOptions", options);
            ILoggerFactory loggerFactory = NullLoggerFactory.Instance;
            var services = new ServiceCollection();
            services.AddSingleton(loggerFactory);
            services.AddSingleton(new HttpClient(new NoSend()));
            services.AddTransient<SequenceElementBuilder>();
            services.AddTransient<ShareUsageBuilder>();
            services.AddTransient<JsonBuilderElementBuilder>();
            services.AddTransient<DeviceDetectionHashEngineBuilder>();
            services.AddTransient<RobotsTxtEngineBuilder>();
            services.AddTransient<IpiOnPremiseEngineBuilder>();
            using var provider = services.BuildServiceProvider();
            IPipeline pipeline;
            try
            {
                pipeline = new FiftyOnePipelineBuilder(loggerFactory, provider)
                    .BuildFromConfiguration(options);
            }
            catch (PipelineConfigurationException ex)
            {
                return new JsonObject { ["error"] = ex.Message };
            }
            using (pipeline)
            {
                var elements = new JsonArray();
                foreach (var element in pipeline.FlowElements)
                {
                    elements.Add(DescribeElement(element, files));
                }
                return new JsonObject
                {
                    ["error"] = null,
                    ["suppressProcessExceptions"] = pipeline.SuppressProcessExceptions,
                    ["concurrent"] = pipeline.IsConcurrent,
                    ["elements"] = elements
                };
            }
        }

        // An element's data key and the settings read back from it.
        private static JsonObject DescribeElement(
            IFlowElement element, IReadOnlyDictionary<string, string> files)
        {
            var result = new JsonObject { ["dataKey"] = element.ElementDataKey };
            if (element is IOnPremiseAspectEngine engine)
            {
                result["dataFile"] = DescribeDataFile(engine.DataFiles[0], files);
            }
            if (element is ShareUsageElement)
            {
                result["settings"] = DescribeShareUsage(element);
                var filter = new JsonObject();
                foreach (var key in UsageFilterKeys)
                {
                    filter[key] = element.EvidenceKeyFilter.Include(key);
                }
                result["evidenceFilter"] = filter;
            }
            return result;
        }

        // The data file settings, with the data file's path given as its
        // placeholder.
        private static JsonObject DescribeDataFile(
            IAspectEngineDataFile file, IReadOnlyDictionary<string, string> files)
        {
            var configuration = file.Configuration;
            var path = files.FirstOrDefault(
                f => f.Value == configuration.DataFilePath).Key
                ?? configuration.DataFilePath;
            return new JsonObject
            {
                ["dataFilePath"] = path,
                ["automaticUpdatesEnabled"] = configuration.AutomaticUpdatesEnabled,
                ["fileSystemWatcherEnabled"] = configuration.FileSystemWatcherEnabled,
                ["updateOnStartup"] = configuration.UpdateOnStartup,
                ["pollingIntervalSeconds"] = configuration.PollingIntervalSeconds,
                ["maxRandomisationSeconds"] = configuration.MaxRandomisationSeconds,
                ["dataUpdateUrl"] = configuration.DataUpdateUrl,
                ["dataUpdateLicenseKeys"] = new JsonArray(configuration
                    .DataUpdateLicenseKeys
                    .Select(k => (JsonNode)JsonValue.Create(k)).ToArray()),
                ["verifyMd5"] = configuration.VerifyMd5,
                ["decompressContent"] = configuration.DecompressContent,
                ["verifyModifiedSince"] = configuration.VerifyModifiedSince,
                ["formattedUrl"] = file.FormattedUri?.AbsoluteUri
            };
        }

        // The usage sharing settings, which ShareUsageBase holds in fields
        // that are not public. The blocked headers, the query string
        // parameters and sharing all evidence are read back through the
        // element's evidence filter instead.
        private static JsonObject DescribeShareUsage(IFlowElement element)
        {
            var queue = Field(element, "<EvidenceCollection>k__BackingField");
            var ignore = new JsonArray();
            foreach (var pair in (IEnumerable)Field(element, "_ignoreDataEvidenceFilter"))
            {
                var kv = (KeyValuePair<string, string>)pair;
                ignore.Add(new JsonArray(kv.Key, kv.Value));
            }
            return new JsonObject
            {
                ["sharePercentage"] = (double)Field(element, "_sharePercentage"),
                ["minimumEntriesPerMessage"] =
                    (int)Field(element, "<MinEntriesPerMessage>k__BackingField"),
                ["maximumQueueSize"] = (int)queue.GetType()
                    .GetProperty("BoundedCapacity").GetValue(queue),
                ["addTimeoutMilliseconds"] = (int)Field(element, "_addTimeout"),
                ["takeTimeoutMilliseconds"] =
                    (int)Field(element, "<TakeTimeout>k__BackingField"),
                ["shareUsageUrl"] =
                    ((Uri)Field(element, "<ShareUsageUri>k__BackingField")).AbsoluteUri,
                ["repeatEvidenceIntervalMinutes"] =
                    ((TimeSpan)Field(element, "_interval")).TotalMinutes,
                ["ignoreFlowDataEvidenceFilter"] = ignore
            };
        }

        // The field `name` of `target` or of a base class, which must exist,
        // so a package that renames it fails the run rather than the answer.
        private static object Field(object target, string name)
        {
            for (var t = target.GetType(); t != null; t = t.BaseType)
            {
                var field = t.GetField(name, BindingFlags.Instance
                    | BindingFlags.Public | BindingFlags.NonPublic
                    | BindingFlags.DeclaredOnly);
                if (field != null)
                {
                    return field.GetValue(target);
                }
            }
            throw new MissingFieldException(target.GetType().Name, name);
        }
    }
}
