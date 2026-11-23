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

// The derived property cases: every case of each case file
// derived-properties ships, run through a pipeline of one source element for
// each data key and the derived property element with the built-in script
// the file names, as the package's own conformance tests build it.
//
//   dotnet run -- derived --cases <folder> --out <jsonl>
//
// parity/README.md ("The derived property comparison") describes the line.
// The case files are read with the package's own parser.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json.Nodes;
using FiftyOne.Pipeline.Core.Data;
using FiftyOne.Pipeline.Core.FlowElements;
using FiftyOne.Pipeline.DerivedProperty.Data;
using FiftyOne.Pipeline.DerivedProperty.FlowElements;
using FiftyOne.Pipeline.Engines.Data;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace FiftyOne.Parity
{
    internal static class DerivedCases
    {
        // The first argument that selects this form.
        public const string Run = "derived";

        // The end of a case file's name.
        private const string CaseFileSuffix = ".cases.yaml";

        private static readonly ILoggerFactory Loggers =
            NullLoggerFactory.Instance;

        public static int RunCases(string[] argv)
        {
            string folder = null, outPath = null;
            for (var i = 0; i + 1 < argv.Length; i += 2)
            {
                switch (argv[i])
                {
                    case "--cases": folder = argv[i + 1]; break;
                    case "--out": outPath = argv[i + 1]; break;
                    default:
                        throw new ArgumentException(
                            $"unknown argument {argv[i]}");
                }
            }
            if (folder == null || outPath == null)
            {
                throw new ArgumentException("--cases and --out are required");
            }
            var files = Directory.GetFiles(folder)
                .Where(f => Path.GetFileName(f).EndsWith(
                    CaseFileSuffix, StringComparison.Ordinal))
                .OrderBy(f => Path.GetFileName(f), StringComparer.Ordinal)
                .ToList();
            if (files.Count == 0)
            {
                throw new ArgumentException($"{folder} holds no case files");
            }
            using var writer = new StreamWriter(outPath, false,
                new UTF8Encoding(false));
            writer.NewLine = "\n";
            foreach (var file in files)
            {
                var document = DerivedScriptParser.Parse(
                    File.ReadAllText(file));
                var script = Text(document.Get("Script"));
                var cases = (DerivedSequence)document.Get("Cases");
                for (var i = 0; i < cases.Items.Count; i++)
                {
                    var line = new JsonObject
                    {
                        // derived-, the script's name in lower case and the
                        // case's place in its file, counting from 1.
                        ["id"] = string.Format(
                            CultureInfo.InvariantCulture,
                            "derived-{0}-{1:00}",
                            script.ToLowerInvariant(),
                            i + 1),
                        ["derived"] = RunCase(
                            script, (DerivedMapping)cases.Items[i])
                    };
                    writer.WriteLine(line.ToJsonString());
                }
            }
            return 0;
        }

        // Runs one case. The line holds the script's source properties as it
        // names them, then the output's value and type, or its no value
        // message and the source properties that message names.
        private static JsonObject RunCase(string scriptName, DerivedMapping entry)
        {
            var element = new DerivedPropertyElementBuilder(Loggers)
                .SetScripts(new List<string> { scriptName })
                .Build();
            var script = element.Scripts[0];
            var names = script.Properties.Select(p => p.Name).ToList();
            var result = new JsonObject
            {
                ["script"] = scriptName,
                ["sourceProperties"] = new JsonArray(
                    names.Select(n => (JsonNode)n).ToArray())
            };

            IPipeline pipeline;
            try
            {
                var builder = new PipelineBuilder(Loggers);
                foreach (var source in BuildSources(
                    script, ReadProperties(entry.Get("Properties"))))
                {
                    builder.AddFlowElement(source);
                }
                builder.AddFlowElement(element);
                pipeline = builder.Build();
            }
            catch (Exception ex)
            {
                result["error"] = ex.Message;
                return result;
            }

            using (pipeline)
            using (var data = pipeline.CreateFlowData())
            {
                string processError = null;
                try
                {
                    data.Process();
                }
                catch (Exception ex)
                {
                    processError = ex.Message;
                }
                object written = null;
                if (data.TryGet(element.ElementDataKey, out var derived))
                {
                    derived.TryGet(script.Output.Name, out written);
                }
                JsonNode type = null, value = null, noValue = null;
                var missing = new JsonArray();
                if (written is IAspectPropertyValue answer)
                {
                    if (answer.HasValue)
                    {
                        type = TypeName(answer.Value);
                        value = ValueOf(answer.Value);
                    }
                    else
                    {
                        noValue = answer.NoValueMessage;
                        foreach (var name in NamedIn(answer.NoValueMessage, names))
                        {
                            missing.Add((JsonNode)name);
                        }
                    }
                }
                result["processError"] = processError;
                result["type"] = type;
                result["value"] = value;
                result["noValue"] = noValue;
                result["missing"] = missing;
            }
            return result;
        }

        // The source properties a no value message names, in the script's
        // order. A name counts when the message holds it between single
        // quotes, ignoring case, as the package's conformance tests look for
        // it.
        private static IEnumerable<string> NamedIn(
            string message,
            IEnumerable<string> names)
        {
            return names.Where(n => message != null && message.IndexOf(
                "'" + n + "'", StringComparison.OrdinalIgnoreCase) >= 0);
        }

        private static string TypeName(object value)
        {
            switch (value)
            {
                case string _: return "string";
                case bool _: return "bool";
                case int _:
                case long _: return "integer";
                case double _:
                case float _: return "double";
                default: return "other";
            }
        }

        private static JsonNode ValueOf(object value)
        {
            switch (value)
            {
                case string s: return s;
                case bool b: return b;
                case int i: return i;
                case long l: return l;
                case double d: return d;
                case float f: return f;
                default:
                    return Convert.ToString(
                        value, CultureInfo.InvariantCulture);
            }
        }

        // One source element for each data key the script or the case names,
        // in the ordinal order of the keys, as the package's conformance tests
        // build them. Each lists every source property of the script under
        // its key, and the case's own properties, so the pipeline check
        // passes, and writes only the values the case gives, so a case that
        // leaves one out has it absent.
        private static List<IFlowElement> BuildSources(
            DerivedScript script,
            IReadOnlyDictionary<string, object> properties)
        {
            var byKey = new Dictionary<string, Dictionary<string, object>>(
                StringComparer.OrdinalIgnoreCase);
            foreach (var property in script.Properties)
            {
                if (byKey.ContainsKey(property.ElementDataKey) == false)
                {
                    byKey.Add(
                        property.ElementDataKey,
                        new Dictionary<string, object>(
                            StringComparer.OrdinalIgnoreCase));
                }
            }
            foreach (var property in properties)
            {
                var dot = property.Key.IndexOf('.');
                var key = property.Key.Substring(0, dot);
                var name = property.Key.Substring(dot + 1);
                if (byKey.TryGetValue(key, out var values) == false)
                {
                    values = new Dictionary<string, object>(
                        StringComparer.OrdinalIgnoreCase);
                    byKey.Add(key, values);
                }
                values[name] = property.Value;
            }

            var sources = new List<IFlowElement>();
            foreach (var key in byKey.Keys.OrderBy(k => k, StringComparer.Ordinal))
            {
                var values = byKey[key];
                var declared = script.Properties
                    .Where(p => string.Equals(
                        p.ElementDataKey, key, StringComparison.OrdinalIgnoreCase))
                    .Select(p => p.PropertyName)
                    .Where(n => values.ContainsKey(n) == false)
                    .ToList();
                sources.Add(new CaseSourceElement(key, values, declared));
            }
            return sources;
        }

        // The properties of one case, keyed as the case wrote them in
        // elementKey.PropertyName form.
        private static Dictionary<string, object> ReadProperties(DerivedNode node)
        {
            var result = new Dictionary<string, object>(
                StringComparer.OrdinalIgnoreCase);
            if (node == null)
            {
                return result;
            }
            var mapping = (DerivedMapping)node;
            foreach (var name in mapping.Names)
            {
                if (name.IndexOf('.') < 0)
                {
                    throw new FormatException(
                        $"'{name}' is not written as elementKey.PropertyName");
                }
                result.Add(name, ReadValue(name, mapping.Get(name)));
            }
            return result;
        }

        // A case writes a value one of four ways, read as the package's
        // conformance tests read them. A plain value arrives as its own type.
        // { String: ... } arrives as text, { NoValue: ... } as a value with
        // that no value message, and a list of Value and Weight pairs as
        // weighted values.
        private static object ReadValue(string property, DerivedNode node)
        {
            if (node is DerivedScalar scalar)
            {
                return scalar.Value;
            }
            if (node is DerivedMapping mapping)
            {
                if (mapping.Has("String"))
                {
                    return Convert.ToString(
                        Value(mapping.Get("String")),
                        CultureInfo.InvariantCulture) ?? string.Empty;
                }
                if (mapping.Has("NoValue"))
                {
                    return new AspectPropertyValue<string>
                    {
                        NoValueMessage = Text(mapping.Get("NoValue"))
                    };
                }
                throw new FormatException(
                    $"'{property}' is written as a mapping that is neither " +
                    "String nor NoValue");
            }
            if (node is DerivedSequence sequence)
            {
                var weighted = new List<WeightedValue<string>>();
                foreach (var item in sequence.Items)
                {
                    if (!(item is DerivedMapping member) ||
                        member.Has("Value") == false ||
                        member.Has("Weight") == false)
                    {
                        throw new FormatException(
                            $"'{property}' is written as a list, so every " +
                            "member needs a Value and a Weight");
                    }
                    weighted.Add(new WeightedValue<string>(
                        Convert.ToUInt16(
                            Value(member.Get("Weight")),
                            CultureInfo.InvariantCulture),
                        Text(member.Get("Value"))));
                }
                return weighted;
            }
            throw new FormatException(
                $"'{property}' is written in a way the runner does not know");
        }

        private static object Value(DerivedNode node)
        {
            return node is DerivedScalar scalar ? scalar.Value : null;
        }

        private static string Text(DerivedNode node)
        {
            if (!(node is DerivedScalar scalar) || scalar.Value == null)
            {
                return null;
            }
            return Convert.ToString(scalar.Value, CultureInfo.InvariantCulture);
        }
    }

    // Writes one case's values under one data key, listing them and the
    // names declared without a value.
    internal sealed class CaseSourceElement :
        FlowElementBase<CaseSourceData, IElementPropertyMetaData>
    {
        private readonly string _key;

        private readonly IReadOnlyDictionary<string, object> _values;

        private readonly IList<IElementPropertyMetaData> _properties;

        private readonly IEvidenceKeyFilter _filter =
            new EvidenceKeyFilterWhitelist(new List<string>());

        public CaseSourceElement(
            string key,
            IReadOnlyDictionary<string, object> values,
            IEnumerable<string> declaredWithoutValues)
            : base(NullLogger<FlowElementBase<
                CaseSourceData, IElementPropertyMetaData>>.Instance)
        {
            _key = key;
            _values = values;
            _properties = values.Keys
                .Concat(declaredWithoutValues)
                .Select(name => (IElementPropertyMetaData)
                    new ElementPropertyMetaData(this, name, typeof(object), true))
                .ToList();
        }

        public override string ElementDataKey => _key;

        public override IEvidenceKeyFilter EvidenceKeyFilter => _filter;

        public override IList<IElementPropertyMetaData> Properties =>
            _properties;

        protected override void ProcessInternal(IFlowData data)
        {
            var elementData = data.GetOrAdd(
                ElementDataKeyTyped,
                pipeline => new CaseSourceData(pipeline));
            foreach (var value in _values)
            {
                elementData[value.Key] = value.Value;
            }
        }

        protected override void ManagedResourcesCleanup() { }

        protected override void UnmanagedResourcesCleanup() { }
    }

    internal sealed class CaseSourceData : ElementDataBase
    {
        public CaseSourceData(IPipeline pipeline)
            : base(NullLogger<ElementDataBase>.Instance, pipeline)
        {
        }
    }
}
