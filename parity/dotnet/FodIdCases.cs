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

// The 51Did cases.
//
//   dotnet run -- fodid-sign --cases <json> --prefix <text> --out <json>
//   dotnet run -- fodid --corpus <json> [--corpus <json>] --out <jsonl>
//
// fodid-sign signs each payload in the cases file with a key made for the
// run and writes the list each language reads. fodid reads one or more such
// lists and writes a line per case with every accessor, the parse status and
// the signature status, or the 51Did metadata. The formats are the Rust
// producer's, as parity/README.md describes.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using FiftyOne.Did.Core.FlowElements;
using FiftyOne.Did.Model;
using FiftyOne.Pipeline.Core.Data.Types;
using FiftyOne.Pipeline.Engines.Data;
using FiftyOne.Pipeline.Engines.FiftyOne.Data;
using Owid.Client;

namespace FiftyOne.Parity
{
    internal static class FodIdCases
    {
        // The mode that signs the cases.
        public const string Sign = "fodid-sign";

        // The mode that reads the signed lists.
        public const string Read = "fodid";

        // The creator domain every payload is signed with.
        private const string Domain = "parity.test";

        // The characters between two spaces in the url-spaced encoding.
        private const int SpacedGroupLength = 20;

        // The date as RFC 3339 in UTC. The envelope holds whole minutes.
        private const string DateFormat = "yyyy-MM-dd'T'HH:mm:ss'Z'";

        // The fields a case of the cases file may have.
        private static readonly HashSet<string> CaseFields = new HashSet<string>
        {
            "id", "about", "payload", "text", "metadata", "encoding",
            "trailing", "tamper"
        };

        // The fields an entry of a signed list may have.
        private static readonly HashSet<string> SignedFields =
            new HashSet<string>
            {
                "id", "about", "form", "input", "publicKey", "tamper"
            };

        private static readonly JsonSerializerOptions Indented =
            new JsonSerializerOptions { WriteIndented = true };

        // fodid-sign: signs each payload of --cases and writes the list, with
        // --prefix before every id.
        public static int SignCases(string[] argv)
        {
            var flags = Flags(argv, "--cases", "--prefix", "--out");
            var casesPath = SingleValue(flags, "--cases");
            var prefix = SingleValue(flags, "--prefix");
            var output = SingleValue(flags, "--out");
            var cases = JsonNode.Parse(File.ReadAllText(casesPath)).AsArray()
                .Select(item => item.AsObject())
                .ToList();
            var repeated = Repeated(cases.Select(Id));
            if (repeated != null)
            {
                throw new ArgumentException(
                    $"{casesPath}: case {repeated} is given more than once");
            }

            using var key = ECDsa.Create(ECCurve.NamedCurves.nistP256);
            var publicKey = key.ExportSubjectPublicKeyInfoPem();
            var creator = new Creator(Domain, key);
            var signed = new JsonArray();
            foreach (var item in cases)
            {
                signed.Add(SignCase(item, prefix, creator, publicKey));
            }

            WriteText(output, signed.ToJsonString(Indented) + "\n");
            Console.Error.WriteLine(
                $"{signed.Count} 51Did cases signed into {output}");
            return 0;
        }

        private static JsonObject SignCase(
            JsonObject item,
            string prefix,
            Creator creator,
            string publicKey)
        {
            var id = Id(item);
            foreach (var pair in item)
            {
                if (CaseFields.Contains(pair.Key) == false)
                {
                    throw new ArgumentException(
                        $"{id}: unknown field {pair.Key}");
                }
            }
            var payload = item["payload"]?.GetValue<string>();
            var text = item["text"]?.GetValue<string>();
            var metadata = item["metadata"]?.GetValue<bool>() ?? false;
            var encoding = item["encoding"]?.GetValue<string>();
            var trailing = item["trailing"]?.GetValue<string>();
            var tamper = item["tamper"]?.GetValue<bool>() ?? false;

            string form;
            string input = null;
            string key = null;
            if (payload != null && text == null && metadata == false)
            {
                var bytes = creator.Create(FromHex(payload)).AsByteArray();
                if (tamper)
                {
                    bytes[bytes.Length - 1] ^= 1;
                }
                if (trailing != null)
                {
                    bytes = bytes.Concat(FromHex(trailing)).ToArray();
                }
                (form, input) = Encode(id, bytes, encoding ?? "standard");
                key = publicKey;
            }
            else if (payload == null && (text != null) != metadata)
            {
                if (encoding != null || trailing != null || tamper)
                {
                    throw new ArgumentException(
                        $"{id}: encoding, trailing and tamper apply to a " +
                        "payload only");
                }
                form = metadata ? "metadata" : "text";
                input = text;
            }
            else
            {
                throw new ArgumentException(
                    $"{id}: a case has exactly one of payload, text and " +
                    "metadata");
            }
            return new JsonObject
            {
                ["id"] = prefix + id,
                ["about"] = item["about"].GetValue<string>(),
                ["form"] = form,
                ["input"] = input,
                ["publicKey"] = key,
                ["tamper"] = tamper
            };
        }

        // The form and input of an envelope given in one of the encodings
        // the cases file names.
        private static (string, string) Encode(
            string id,
            byte[] bytes,
            string encoding)
        {
            switch (encoding)
            {
                case "standard":
                    return ("text", Convert.ToBase64String(bytes));
                case "url":
                    return ("text", UrlSafe(bytes));
                case "url-surrounded":
                    return ("text", " " + UrlSafe(bytes) + "\n");
                case "url-spaced":
                    return ("text", Spaced(UrlSafe(bytes)));
                case "bytes":
                    return ("bytes", Hex(bytes));
                default:
                    throw new ArgumentException(
                        $"{id}: unknown encoding {encoding}");
            }
        }

        // The URL-safe alphabet with no padding, written here rather than
        // with the package under test.
        private static string UrlSafe(byte[] bytes)
        {
            return Convert.ToBase64String(bytes)
                .Replace('+', '-').Replace('/', '_').TrimEnd('=');
        }

        // The text with a space between each group of SpacedGroupLength
        // characters.
        private static string Spaced(string text)
        {
            var output = new StringBuilder();
            for (var i = 0; i < text.Length; i++)
            {
                if (i > 0 && i % SpacedGroupLength == 0)
                {
                    output.Append(' ');
                }
                output.Append(text[i]);
            }
            return output.ToString();
        }

        // fodid: reads every case of each --corpus list and writes a line for
        // each to --out.
        public static int ReadCases(string[] argv)
        {
            var flags = Flags(argv, "--corpus", "--out");
            if (flags.TryGetValue("--corpus", out var corpora) == false)
            {
                throw new ArgumentException("--corpus is required");
            }
            var output = SingleValue(flags, "--out");
            var entries = corpora
                .SelectMany(path => JsonNode.Parse(File.ReadAllText(path))
                    .AsArray()
                    .Select(item => item.AsObject()))
                .ToList();
            var repeated = Repeated(entries.Select(Id));
            if (repeated != null)
            {
                throw new ArgumentException(
                    $"case {repeated} is given more than once");
            }

            var lines = new StringBuilder();
            foreach (var entry in entries)
            {
                lines.Append(Line(entry).ToJsonString()).Append('\n');
            }
            WriteText(output, lines.ToString());
            Console.Error.WriteLine(
                $"{entries.Count} 51Did cases written to {output}");
            return 0;
        }

        private static JsonObject Line(JsonObject entry)
        {
            var id = Id(entry);
            foreach (var pair in entry)
            {
                if (SignedFields.Contains(pair.Key) == false)
                {
                    throw new ArgumentException(
                        $"{id}: unknown field {pair.Key}");
                }
            }
            var input = entry["input"]?.GetValue<string>();
            var publicKey = entry["publicKey"]?.GetValue<string>();
            bool parsed;
            FodId fodId;
            FodIdParseStatus status;
            var form = entry["form"]?.GetValue<string>();
            switch (form)
            {
                case "metadata":
                    return new JsonObject
                    {
                        ["id"] = id,
                        ["metadata"] = Metadata()
                    };
                case "text":
                    parsed = FodId.TryParse(Input(id, input), out fodId,
                        out status);
                    break;
                case "bytes":
                    parsed = FodId.TryParse(FromHex(Input(id, input)),
                        out fodId, out status);
                    break;
                default:
                    throw new ArgumentException($"{id}: unknown form {form}");
            }
            return new JsonObject
            {
                ["id"] = id,
                ["fodid"] = Fields(parsed ? fodId : null, status, publicKey)
            };
        }

        private static string Input(string id, string input)
        {
            return input ?? throw new ArgumentException($"{id}: no input");
        }

        // The parse status, then every accessor's value, each null when the
        // 51Did was not read. The signature status is null for a case with
        // no key.
        private static JsonObject Fields(
            FodId fodId,
            FodIdParseStatus status,
            string publicKey)
        {
            var accessors = new (string, Func<FodId, JsonNode>)[]
            {
                ("type", f => f.Type.ToString()),
                ("usage", f => f.Usage.ToString()),
                ("idUsage", f => f.Usage.IdUsage()),
                ("usageIsIndirect", f => f.UsageIsIndirect),
                ("licenseId", f => f.LicenseId),
                ("matchKey", f => Hex(f.MatchKey)),
                ("terms", f => f.Terms),
                ("base64Url", f => f.AsBase64Url()),
                ("version", f => (int)f.Version),
                ("domain", f => f.Domain),
                // The reader dates an envelope from a base in UTC.
                ("date", f => f.Date.ToString(
                    DateFormat, CultureInfo.InvariantCulture)),
                ("payload", f => Hex(f.Payload)),
                ("signature", f => Hex(f.Signature)),
                ("base64", f => f.AsBase64()),
                ("signatureStatus", f => publicKey == null
                    ? null
                    : f.SignatureStatus(publicKey).ToString())
            };
            var fields = new JsonObject { ["status"] = status.ToString() };
            foreach (var (name, accessor) in accessors)
            {
                fields[name] = fodId == null ? null : Field(fodId, accessor);
            }
            return fields;
        }

        // An accessor's value, or what it threw with a dotnet: prefix, so it
        // shows as a difference.
        private static JsonNode Field(
            FodId fodId,
            Func<FodId, JsonNode> accessor)
        {
            try
            {
                return accessor(fodId);
            }
            catch (Exception e)
            {
                return $"dotnet:{e.GetType().Name}: {e.Message}";
            }
        }

        // The 51Did component and its properties in list order, as the cloud
        // engine builds them. Nothing reads the engine, so none is given.
        private static JsonObject Metadata()
        {
            var component = DidBaseEnginePropertiesBuilder
                .BuildComponentMetaData(null, true);
            return new JsonObject
            {
                ["name"] = component.Name,
                ["componentId"] = component.ComponentId,
                ["properties"] = new JsonArray(component.GetProperties()
                    .Select(property => (JsonNode)PropertyMetadata(property))
                    .ToArray())
            };
        }

        private static JsonObject PropertyMetadata(
            IFiftyOneAspectPropertyMetaData property)
        {
            return new JsonObject
            {
                ["name"] = property.Name,
                ["category"] = property.Category,
                ["component"] = property.Component.Name,
                ["componentId"] = property.Component.ComponentId,
                ["type"] = TypeName(property.Type),
                ["defaultValue"] = property.DefaultValue?.Name,
                ["available"] = property.Available,
                ["dataTiers"] = Strings(property.DataTiersWherePresent),
                ["description"] = property.Description,
                ["displayOrder"] = property.DisplayOrder,
                ["mandatory"] = property.Mandatory,
                ["list"] = property.List,
                ["obsolete"] = property.Obsolete,
                ["show"] = property.Show,
                ["showValues"] = property.ShowValues,
                ["url"] = property.Url,
                ["valueCount"] = property.GetValues().Count(),
                ["delayExecution"] = property.DelayExecution,
                // .NET leaves the list null when a property names no
                // evidence, where the Rust list is empty. Both mean none.
                ["evidenceProperties"] = Strings(property.EvidenceProperties)
            };
        }

        private static JsonArray Strings(IEnumerable<string> values)
        {
            return new JsonArray((values ?? Enumerable.Empty<string>())
                .Select(value => (JsonNode)value)
                .ToArray());
        }

        // The type a property's values have, by the names the other lines
        // use. An aspect property value is named by the type it holds.
        internal static string TypeName(Type type)
        {
            if (type.IsGenericType &&
                type.GetGenericTypeDefinition() ==
                    typeof(IAspectPropertyValue<>))
            {
                type = type.GetGenericArguments()[0];
            }
            if (type == typeof(string)) return "string";
            if (type == typeof(bool)) return "bool";
            if (type == typeof(int) || type == typeof(long)) return "integer";
            if (type == typeof(double)) return "double";
            if (type == typeof(float)) return "float";
            if (type == typeof(JavaScript)) return "javascript";
            if (typeof(IEnumerable<string>).IsAssignableFrom(type))
            {
                return "stringList";
            }
            return "dotnet:" + type.Name;
        }

        private static string Hex(byte[] bytes)
        {
            return Convert.ToHexString(bytes).ToLowerInvariant();
        }

        // The bytes written as hex in the text, ignoring white space.
        private static byte[] FromHex(string text)
        {
            return Convert.FromHexString(new string(
                text.Where(c => char.IsWhiteSpace(c) == false).ToArray()));
        }

        private static string Id(JsonObject item)
        {
            return item["id"]?.GetValue<string>() ??
                throw new ArgumentException("a case has no id");
        }

        // The first id given twice, or null.
        private static string Repeated(IEnumerable<string> ids)
        {
            var seen = new HashSet<string>();
            return ids.FirstOrDefault(id => seen.Add(id) == false);
        }

        // The values given for each flag. Every flag takes a value, and only
        // the flags allowed are accepted.
        private static Dictionary<string, List<string>> Flags(
            string[] argv,
            params string[] allowed)
        {
            var values = new Dictionary<string, List<string>>();
            for (var i = 0; i < argv.Length; i += 2)
            {
                if (allowed.Contains(argv[i]) == false)
                {
                    throw new ArgumentException($"unknown argument {argv[i]}");
                }
                if (i + 1 == argv.Length)
                {
                    throw new ArgumentException($"{argv[i]} needs a value");
                }
                if (values.TryGetValue(argv[i], out var list) == false)
                {
                    values[argv[i]] = list = new List<string>();
                }
                list.Add(argv[i + 1]);
            }
            return values;
        }

        // The one value given for the flag.
        private static string SingleValue(
            Dictionary<string, List<string>> values,
            string flag)
        {
            if (values.TryGetValue(flag, out var list) == false)
            {
                throw new ArgumentException($"{flag} is required");
            }
            if (list.Count != 1)
            {
                throw new ArgumentException($"{flag} may be given once");
            }
            return list[0];
        }

        private static void WriteText(string path, string text)
        {
            File.WriteAllText(path, text, new UTF8Encoding(false));
        }
    }
}
