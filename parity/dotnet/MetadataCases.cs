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

// The metadata cases: each on-premise engine's properties, components and
// values, and what an engine opened with some properties gives when the
// others are read.
//
//   dotnet run -- metadata --cases <json> --dd <hash file> --ipi <ipi file> --out <jsonl>
//
// The Rust producer writes the same line. parity/README.md ("The metadata
// comparison") describes the cases and the line.

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Text;
using System.Text.Json.Nodes;
using FiftyOne.DeviceDetection.Hash.Engine.OnPremise.FlowElements;
using FiftyOne.IpIntelligence.Engine.OnPremise.Data;
using FiftyOne.IpIntelligence.Engine.OnPremise.FlowElements;
using FiftyOne.Pipeline.Core.Data;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.Engines;
using FiftyOne.Pipeline.Engines.FiftyOne.Data;
using FiftyOne.Pipeline.Engines.FiftyOne.FlowElements;
using FiftyOne.Pipeline.Engines.FlowElements;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    internal static class MetadataCases
    {
        // The first argument that selects this form.
        public const string Run = "metadata";

        // The engine of a case for each on-premise engine.
        private const string Device = "device";
        private const string Ip = "ip";

        private static readonly ILoggerFactory Loggers =
            NullLoggerFactory.Instance;

        public static int RunCases(string[] argv)
        {
            string cases = null, dd = null, ipi = null, outPath = null;
            for (var i = 0; i + 1 < argv.Length; i += 2)
            {
                switch (argv[i])
                {
                    case "--cases": cases = argv[i + 1]; break;
                    case "--dd": dd = argv[i + 1]; break;
                    case "--ipi": ipi = argv[i + 1]; break;
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
            var items = JsonNode.Parse(File.ReadAllText(cases)).AsArray();
            using (var writer = new StreamWriter(outPath, false,
                new UTF8Encoding(false)))
            {
                writer.NewLine = "\n";
                foreach (var item in items)
                {
                    writer.WriteLine(
                        RunCase(item.AsObject(), dd, ipi).ToJsonString());
                }
            }
            Console.Error.WriteLine(
                $"{items.Count} metadata cases written to {outPath}");
            return 0;
        }

        private static JsonObject RunCase(
            JsonObject item,
            string dd,
            string ipi)
        {
            var id = item["id"].GetValue<string>();
            var names = Names(item["properties"]);
            var kind = item["engine"]?.GetValue<string>();
            IFiftyOneAspectEngine engine =
                kind switch
                {
                    Device => BuildDevice(dd, names),
                    Ip => BuildIp(ipi, names),
                    var other => throw new ArgumentException(
                        $"{id}: unknown engine {other}")
                };
            try
            {
                var metadata = Metadata(
                    engine,
                    item["valueLimit"].GetValue<int>(),
                    kind == Device);
                var (processError, properties) = Read(engine, item);
                return new JsonObject
                {
                    ["id"] = id,
                    ["processError"] = processError,
                    ["properties"] = properties,
                    ["engineMetadata"] = metadata
                };
            }
            finally
            {
                engine.Dispose();
            }
        }

        // The HighPerformance profile, as the engines' pipeline has it.
        private static IFiftyOneAspectEngine BuildDevice(
            string file,
            IReadOnlyList<string> names)
        {
            var builder = new DeviceDetectionHashEngineBuilder(Loggers)
                .SetPerformanceProfile(PerformanceProfiles.HighPerformance)
                .SetAutoUpdate(false)
                .SetDataFileSystemWatcher(false)
                .SetDataUpdateOnStartup(false);
            foreach (var name in names)
            {
                builder.SetProperty(name);
            }
            return builder.Build(file, false);
        }

        // The LowMemory profile, as the engines' pipeline has it.
        private static IFiftyOneAspectEngine BuildIp(
            string file,
            IReadOnlyList<string> names)
        {
            var builder = new IpiOnPremiseEngineBuilder(Loggers)
                .SetPerformanceProfile(PerformanceProfiles.LowMemory)
                .SetAutoUpdate(false)
                .SetDataFileSystemWatcher(false)
                .SetDataUpdateOnStartup(false);
            foreach (var name in names)
            {
                builder.SetProperty(name);
            }
            return builder.Build(file, false);
        }

        // The properties the case's read names, from the data of a request
        // to the engine alone, keyed by the engine's data key and the
        // lowercase name, with no request made when it names none.
        private static (string, JsonObject) Read(
            IFiftyOneAspectEngine engine,
            JsonObject item)
        {
            var elements = new JsonObject();
            var names = Names(item["read"]);
            if (names.Count == 0)
            {
                return (null, elements);
            }
            using var pipeline = new PipelineBuilder(Loggers)
                .AddFlowElement(engine)
                .SetAutoDisposeElements(false)
                .SetSuppressProcessExceptions(true)
                .Build();
            using var data = pipeline.CreateFlowData();
            foreach (var pair in item["evidence"]?.AsObject() ??
                new JsonObject())
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
            var key = engine.ElementDataKey;
            if (data.ElementDataAsDictionary().TryGetValue(key, out var found)
                == false || found is IElementData elementData == false)
            {
                elements[key] = null;
                return (processError, elements);
            }
            var values = new JsonObject();
            foreach (var name in names)
            {
                values[name.ToLowerInvariant()] =
                    Program.Read(elementData, name);
            }
            elements[key] = values;
            return (processError, elements);
        }

        // The engine's tier, its properties keyed by name and its components
        // keyed by name, each list's order kept beside it, with up to limit
        // values of each property, every one when it is 0. When lastValues
        // is true, up to limit of the values after those follow, taken from
        // the end of the list. An IP Intelligence property's last values are
        // left out, as .NET reaches them only by reading every value before
        // them, millions for some properties.
        private static JsonObject Metadata(
            IFiftyOneAspectEngine engine,
            int limit,
            bool lastValues)
        {
            var properties = ((IAspectEngine)engine).Properties
                .Cast<IFiftyOneAspectPropertyMetaData>()
                .ToList();
            var byName = new JsonObject();
            foreach (var property in properties)
            {
                byName[property.Name] =
                    PropertyMetadata(property, limit, lastValues);
            }
            var components = engine.Components.ToList();
            var componentsByName = new JsonObject();
            foreach (var component in components)
            {
                var entry = new JsonObject
                {
                    ["id"] = component.ComponentId
                };
                Attempt(entry, "defaultProfileId", () =>
                {
                    var profile = component.DefaultProfile;
                    return profile == null
                        ? null : JsonValue.Create(profile.ProfileId);
                });
                Attempt(entry, "properties", () => Strings(
                    component.GetProperties().Select(p => p.Name)));
                componentsByName[component.Name] = entry;
            }
            return new JsonObject
            {
                ["dataSourceTier"] = engine.DataSourceTier,
                ["propertyNames"] = Strings(properties.Select(p => p.Name)),
                ["properties"] = byName,
                ["componentNames"] = Strings(components.Select(c => c.Name)),
                ["components"] = componentsByName
            };
        }

        private static JsonObject PropertyMetadata(
            IFiftyOneAspectPropertyMetaData property,
            int limit,
            bool lastValues)
        {
            var entry = new JsonObject
            {
                ["type"] = TypeName(property.Type),
                ["category"] = property.Category,
                ["description"] = property.Description,
                ["url"] = property.Url,
                ["displayOrder"] = property.DisplayOrder,
                ["mandatory"] = property.Mandatory,
                ["list"] = property.List,
                ["obsolete"] = property.Obsolete,
                ["show"] = property.Show,
                ["showValues"] = property.ShowValues,
                ["available"] = property.Available,
                ["dataTiers"] = Strings(property.DataTiersWherePresent)
            };
            Attempt(entry, "component", () =>
            {
                var component = property.Component;
                return component == null ? null : new JsonObject
                {
                    ["id"] = component.ComponentId,
                    ["name"] = component.Name
                };
            });
            Attempt(entry, "defaultValue",
                () => Value(property.DefaultValue));
            // An IP Intelligence data file property counts its values
            // without reading them, as one can hold millions. Any other
            // property's values are counted as they are read, and the last
            // ones past the first limit are kept as they go by.
            long count = 0;
            var shown = new JsonArray();
            JsonArray last = null;
            Attempt(entry, "valueCount", () =>
            {
                if (property is PropertyMetaDataIpi ipi)
                {
                    count = (long)ipi.GetValuesCount();
                    var first = limit == 0
                        ? ipi.GetValues()
                        : ipi.GetValues().Take(limit);
                    foreach (var value in first)
                    {
                        shown.Add(Value(value));
                    }
                }
                else
                {
                    var tail = new Queue<JsonNode>();
                    foreach (var value in property.GetValues())
                    {
                        if (limit == 0 || count < limit)
                        {
                            shown.Add(Value(value));
                        }
                        else if (lastValues)
                        {
                            tail.Enqueue(Value(value));
                            if (tail.Count > limit)
                            {
                                tail.Dequeue();
                            }
                        }
                        count++;
                    }
                    if (lastValues)
                    {
                        last = new JsonArray(tail.ToArray());
                    }
                }
                return JsonValue.Create(count);
            });
            entry["values"] = shown;
            if (last != null)
            {
                entry["lastValues"] = last;
            }
            return entry;
        }

        private static JsonNode Value(IValueMetaData value)
        {
            return value == null ? null : new JsonObject
            {
                ["name"] = value.Name,
                ["description"] = value.Description,
                ["url"] = value.Url
            };
        }

        // Sets entry[name] to what read gives. When reading fails, the
        // exception's type is written under name with "Error" added instead,
        // so the failure shows as a difference rather than stopping the run.
        private static void Attempt(
            JsonObject entry,
            string name,
            Func<JsonNode> read)
        {
            try
            {
                entry[name] = read();
            }
            catch (Exception e)
            {
                entry[name + "Error"] = e.GetType().Name;
            }
        }

        // The type a property's values have, by the names the other lines
        // use. The crates have no type for an IP address or a weighted list.
        private static string TypeName(Type type)
        {
            if (type == typeof(IPAddress))
            {
                return "ipAddress";
            }
            if (type.Name == "WktString")
            {
                return "wktString";
            }
            if (type.IsGenericType &&
                type.GetGenericTypeDefinition() == typeof(IReadOnlyList<>) &&
                type.GetGenericArguments()[0].IsGenericType &&
                type.GetGenericArguments()[0].GetGenericTypeDefinition() ==
                    typeof(IWeightedValue<>))
            {
                return "weightedList";
            }
            return FodIdCases.TypeName(type);
        }

        private static JsonArray Strings(IEnumerable<string> values)
        {
            return new JsonArray((values ?? Enumerable.Empty<string>())
                .Select(value => (JsonNode)value)
                .ToArray());
        }

        private static IReadOnlyList<string> Names(JsonNode node)
        {
            return node?.AsArray()
                .Select(name => name.GetValue<string>())
                .ToList() ?? new List<string>();
        }
    }
}
