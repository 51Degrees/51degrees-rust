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

// The TAC and Native cases: requests to the TAC and Native engines, in a
// pipeline with the on-premise Device Detection engine, whose data file
// holds the profiles.
//
//   dotnet run -- property-keyed --corpus <json> --dd <hash file> --out <jsonl>
//
// The corpus is the one the Rust producer's property-keyed-cases form
// writes. parity/README.md ("The TAC and Native comparison") describes the
// line.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json.Nodes;
using FiftyOne.DeviceDetection;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.DeviceDetection.PropertyKeyed.Data;
using FiftyOne.DeviceDetection.PropertyKeyed.FlowElements;
using FiftyOne.Pipeline.Core.Data;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.Engines;
using FiftyOne.Pipeline.Engines.Data;
using FiftyOne.Pipeline.JsonBuilder.Data;
using FiftyOne.Pipeline.JsonBuilder.FlowElement;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    internal static class PropertyKeyedCases
    {
        // The first argument that selects this form.
        public const string Run = "property-keyed";

        // The data key both engines write under.
        private const string HardwareKey = "hardware";

        // The property each profile holds its id in.
        private const string ProfileIdProperty = "ProfileId";

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
            var engines = new Engines(device, false);
            var withJson = new Engines(device, true);

            var found = 0;
            using (var writer = new StreamWriter(outPath, false,
                new UTF8Encoding(false)))
            {
                writer.NewLine = "\n";
                foreach (var item in cases)
                {
                    var json = item["json"]?.GetValue<bool>() == true;
                    var line = RunCase(
                        json ? withJson : engines,
                        engines,
                        item.AsObject());
                    if (line["profileIds"] is JsonArray ids && ids.Count > 0)
                    {
                        found++;
                    }
                    writer.WriteLine(line.ToJsonString());
                }
            }
            Console.Error.WriteLine(
                $"{cases.Count} TAC and Native cases written to {outPath}, " +
                $"{found} of them finding profiles");
            engines.Pipeline.Dispose();
            return 0;
        }

        // Runs one case through chosen's pipeline. The metadata comes from
        // engines, the pipeline without the JSON builder.
        private static JsonObject RunCase(
            Engines chosen,
            Engines engines,
            JsonObject item)
        {
            using var data = chosen.Pipeline.CreateFlowData();
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
            var warnings = new JsonArray();
            foreach (var warning in data.GetWarnings())
            {
                warnings.Add(new JsonObject
                {
                    ["element"] = warning.FlowElement?.ElementDataKey,
                    ["message"] = warning.Message
                });
            }

            var hardware = Hardware(data);
            JsonArray profileIds = null;
            if (hardware != null)
            {
                profileIds = new JsonArray(
                    hardware.Profiles.Select(ProfileId).ToArray());
            }
            JsonObject properties = null;
            if (item["profileId"] != null)
            {
                var wanted = item["profileId"].GetValue<ulong>();
                var profile = hardware?.Profiles.FirstOrDefault(p =>
                    ProfileId(p) is JsonValue id &&
                    id.TryGetValue<ulong>(out var got) &&
                    got == wanted);
                properties = new JsonObject
                {
                    ["device"] = profile == null
                        ? null
                        : ProfileValues(profile, engines)
                };
            }
            JsonNode hardwareJson = null;
            if (item["json"]?.GetValue<bool>() == true)
            {
                var text = Try(() =>
                    data.Get<IJsonBuilderElementData>()?.Json);
                var node = text == null
                    ? null
                    : JsonNode.Parse(text)?[HardwareKey];
                hardwareJson = node == null
                    ? null
                    : JsonNode.Parse(node.ToJsonString());
            }
            var engine = item["engine"]?.GetValue<bool>() == true;
            return new JsonObject
            {
                ["id"] = item["id"].GetValue<string>(),
                ["processError"] = processError,
                ["errors"] = errors,
                ["warnings"] = warnings,
                ["profileIds"] = profileIds,
                ["properties"] = properties,
                ["hardwareJson"] = hardwareJson,
                ["propertyKeyedEngines"] = engine
                    ? new JsonArray(engines.Both
                        .Select(e => (JsonNode)Metadata(e))
                        .ToArray())
                    : null,
                ["indexedValues"] = engine ? IndexedValues(engines) : null
            };
        }

        // The engines' data, or null when neither wrote any.
        private static IMultiDeviceData Hardware(IFlowData data)
        {
            return data.ElementDataAsDictionary().TryGetValue(
                HardwareKey, out var found)
                ? found as IMultiDeviceData
                : null;
        }

        // The id a profile holds, as a number, or as text when it is not
        // one.
        private static JsonNode ProfileId(IDeviceData profile)
        {
            string text;
            try
            {
                text = (profile[ProfileIdProperty] as IAspectPropertyValue)?
                    .Value?.ToString();
            }
            catch (Exception)
            {
                return null;
            }
            if (text == null)
            {
                return null;
            }
            return ulong.TryParse(text, NumberStyles.None,
                CultureInfo.InvariantCulture, out var id)
                ? JsonValue.Create(id)
                : JsonValue.Create(text);
        }

        // Each property the engines list for a profile, keyed by its
        // lowercase name, read as the engines' pipeline lines read a
        // property.
        private static JsonObject ProfileValues(
            IDeviceData profile,
            Engines engines)
        {
            var values = new JsonObject();
            foreach (var name in ItemNames(engines))
            {
                values[name.ToLowerInvariant()] = Program.Read(profile, name);
            }
            return values;
        }

        // The properties the TAC engine lists for each profile, or the
        // Native engine's when the TAC engine lists none.
        private static IReadOnlyList<string> ItemNames(Engines engines)
        {
            return engines.Both
                .Select(engine => engine.Properties
                    .SelectMany(property => property.ItemProperties ??
                        Enumerable.Empty<IElementPropertyMetaData>())
                    .Select(item => item.Name)
                    .ToList())
                .FirstOrDefault(names => names.Count > 0) ??
                new List<string>();
        }

        // The engine's data key, data tier, evidence keys in ordinal order
        // and properties in list order, each with its item properties.
        private static JsonObject Metadata(PropertyKeyedDeviceBaseEngine engine)
        {
            var keys = engine.DataSet.EvidenceKeyFilter.Whitelist.Keys
                .OrderBy(key => key, StringComparer.Ordinal)
                .Select(key => (JsonNode)key)
                .ToArray();
            var properties = engine.Properties
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
                    ["description"] = property.Description,
                    ["itemProperties"] = new JsonArray(
                        (property.ItemProperties ??
                            Enumerable.Empty<IElementPropertyMetaData>())
                        .Select(item => (JsonNode)new JsonObject
                        {
                            ["name"] = item.Name,
                            ["category"] = item.Category,
                            ["type"] = FodIdCases.TypeName(item.Type),
                            ["available"] = item.Available
                        })
                        .ToArray())
                })
                .ToArray();
            return new JsonObject
            {
                ["dataKey"] = engine.ElementDataKey,
                ["dataSourceTier"] = engine.DataSourceTier,
                ["evidenceKeys"] = new JsonArray(keys),
                ["properties"] = new JsonArray(properties)
            };
        }

        // For each engine whose key property the data file has, each value
        // of that property its index holds, with how many profiles it
        // lists for the value.
        private static JsonObject IndexedValues(Engines engines)
        {
            var all = new JsonObject();
            foreach (var engine in engines.Both)
            {
                foreach (var index in engine.DataSet.PropertyIndexes)
                {
                    var counts = new JsonObject();
                    foreach (var pair in index.ValueData
                        .OrderBy(pair => pair.Key, StringComparer.Ordinal))
                    {
                        counts[pair.Key] = pair.Value.Count;
                    }
                    all[index.MetaData.Name] = counts;
                }
            }
            return all;
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

        // The Device Detection engine, then the TAC and Native engines, and
        // the JSON builder last when json is set. .NET's engines read the
        // data set of the first pipeline they join, so each pipeline has
        // engines of its own. Only the pipeline without the JSON builder
        // disposes the Device Detection engine they share.
        private sealed class Engines
        {
            public readonly TacEngine Tac;
            public readonly NativeEngine Native;
            public readonly IPipeline Pipeline;

            public Engines(DeviceDetectionHashEngine device, bool json)
            {
                Tac = new TacEngineBuilder(Loggers).Build();
                Native = new NativeEngineBuilder(Loggers).Build();
                var builder = new PipelineBuilder(Loggers)
                    .AddFlowElement(device)
                    .AddFlowElement(Tac)
                    .AddFlowElement(Native);
                if (json)
                {
                    builder = builder.AddFlowElement(
                        new JsonBuilderElementBuilder(Loggers).Build());
                }
                Pipeline = builder
                    .SetSuppressProcessExceptions(true)
                    .SetAutoDisposeElements(json == false)
                    .Build();
            }

            // The two engines, the TAC engine first.
            public IReadOnlyList<PropertyKeyedDeviceBaseEngine> Both =>
                new PropertyKeyedDeviceBaseEngine[] { Tac, Native };
        }
    }
}
