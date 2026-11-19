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

// The robots.txt cases: requests to the robots.txt engine, in a pipeline
// with the on-premise Device Detection engine, whose data file holds the
// crawlers and the usages.
//
//   dotnet run -- robots-txt --corpus <json> --dd <hash file> --out <jsonl>
//
// The corpus is the one the Rust producer's robots-txt-cases form writes.
// parity/README.md ("The robots.txt comparison") describes the line.

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json.Nodes;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.DeviceDetection.RobotsTxt.FlowElements;
using FiftyOne.DeviceDetection.RobotsTxt.Services;
using FiftyOne.Pipeline.Core.Data;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.Engines;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    internal static class RobotsTxtCases
    {
        // The first argument that selects this form.
        public const string Run = "robots-txt";

        // The source id the resolver resolves, compared ignoring case.
        private const string KnownId = "parity-tdl";

        // The URI KnownId resolves to.
        private const string KnownUri = "https://tdl.parity.test/terms/3.txt";

        // The source id the resolver knows but cannot resolve, as when a
        // lookup fails with no earlier answer.
        private const string UnresolvedId = "parity-unresolved";

        private static readonly ILoggerFactory Loggers =
            NullLoggerFactory.Instance;

        public static int RunCases(string[] argv)
        {
            string corpus = null, dataFile = null, outPath = null;
            for (var i = 0; i + 1 < argv.Length; i += 2)
            {
                switch (argv[i])
                {
                    case "--corpus": corpus = argv[i + 1]; break;
                    case "--dd": dataFile = argv[i + 1]; break;
                    case "--out": outPath = argv[i + 1]; break;
                    default:
                        throw new ArgumentException(
                            $"unknown argument {argv[i]}");
                }
            }
            if (corpus == null || dataFile == null || outPath == null)
            {
                throw new ArgumentException(
                    "--corpus, --dd and --out are required");
            }
            var cases = JsonNode.Parse(File.ReadAllText(corpus)).AsArray();
            var device = new DeviceDetectionHashEngineBuilder(Loggers)
                .SetPerformanceProfile(PerformanceProfiles.LowMemory)
                .SetAutoUpdate(false)
                .SetDataFileSystemWatcher(false)
                .SetDataUpdateOnStartup(false)
                .Build(dataFile, false);
            var robots = new RobotsTxtEngineBuilder(Loggers)
                .SetTdlSourceResolver(new ParityResolver())
                .Build();
            var pipeline = new PipelineBuilder(Loggers)
                .AddFlowElement(device)
                .AddFlowElement(robots)
                .SetSuppressProcessExceptions(true)
                .Build();

            using (var writer = new StreamWriter(outPath, false,
                new UTF8Encoding(false)))
            {
                writer.NewLine = "\n";
                foreach (var item in cases)
                {
                    writer.WriteLine(
                        RunCase(pipeline, robots, item.AsObject())
                            .ToJsonString());
                }
            }
            Console.Error.WriteLine(
                $"{cases.Count} robots.txt cases written to {outPath}, " +
                $"with {Whitelist(robots).Count - 1} usage keys");
            pipeline.Dispose();
            return 0;
        }

        // Runs one case. The texts are null when the engine wrote no data,
        // or the property has no value.
        private static JsonObject RunCase(
            IPipeline pipeline,
            RobotsTxtEngine robots,
            JsonObject item)
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
            var errors = new JsonArray();
            foreach (var error in data.Errors ?? Enumerable.Empty<IFlowError>())
            {
                errors.Add(new JsonObject
                {
                    ["element"] = error.FlowElement?.ElementDataKey,
                    ["message"] = error.ExceptionData?.Message
                });
            }
            var engine = item["engine"]?.GetValue<bool>() == true;
            return new JsonObject
            {
                ["id"] = item["id"].GetValue<string>(),
                ["processError"] = processError,
                ["errors"] = errors,
                ["robotsTxtEngine"] = engine ? Metadata(robots) : null,
                ["plainText"] = Try(() =>
                    data.GetFromElement(robots).PlainText.Value),
                ["annotatedText"] = Try(() =>
                    data.GetFromElement(robots).AnnotatedText.Value)
            };
        }

        // The engine's data key, data tier, evidence keys in ordinal order
        // and properties in list order.
        private static JsonObject Metadata(RobotsTxtEngine robots)
        {
            var keys = Whitelist(robots).Keys
                .OrderBy(key => key, StringComparer.Ordinal)
                .Select(key => (JsonNode)key)
                .ToArray();
            var properties = robots.Properties
                .Select(property => (JsonNode)new JsonObject
                {
                    ["name"] = property.Name,
                    ["category"] = property.Category,
                    ["type"] = FodIdCases.TypeName(property.Type),
                    ["available"] = property.Available,
                    ["dataTiers"] = new JsonArray(
                        (property.DataTiersWherePresent ??
                            Enumerable.Empty<string>())
                        .Select(tier => (JsonNode)tier)
                        .ToArray()),
                    ["description"] = property.Description
                })
                .ToArray();
            return new JsonObject
            {
                ["dataKey"] = robots.ElementDataKey,
                ["dataSourceTier"] = robots.DataSourceTier,
                ["evidenceKeys"] = new JsonArray(keys),
                ["properties"] = new JsonArray(properties)
            };
        }

        // The keys the engine accepts, being one for each usage and the
        // Terms Document Locator key.
        private static IReadOnlyDictionary<string, int> Whitelist(
            RobotsTxtEngine robots)
        {
            return ((EvidenceKeyFilterWhitelist)robots.EvidenceKeyFilter)
                .Whitelist;
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

        // Resolves KnownId and knows UnresolvedId, so no case needs the
        // network. The Rust producer's resolver answers the same.
        private sealed class ParityResolver : ITdlSourceResolver
        {
            public bool IsKnown(string id)
            {
                return string.Equals(
                        id, KnownId, StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(
                        id, UnresolvedId, StringComparison.OrdinalIgnoreCase);
            }

            public Uri Resolve(string id)
            {
                return string.Equals(
                    id, KnownId, StringComparison.OrdinalIgnoreCase)
                    ? new Uri(KnownUri)
                    : null;
            }
        }
    }
}
