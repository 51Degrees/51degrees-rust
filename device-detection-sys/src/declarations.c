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

/*
 * Compiled by build.rs to check the C functions and structures declared by
 * hand in src/lib.rs and in fiftyone-common-sys. Rust cannot see a C header, so
 * a changed parameter list or structure would still build and then pass the
 * wrong arguments or read the wrong memory. Declaring a function again with
 * different parameters is an error in C, and so is an array of negative size,
 * which the macros below declare when a structure stops matching, so the build
 * fails instead.
 *
 * Each declaration here must match its Rust declaration. The structures use the
 * C field names so that each field can be checked by name. A new C field that
 * fits in existing padding moves nothing, so these checks cannot see one.
 */

#include <stddef.h>

#include "hash/hash.h"

#define CHECK_SIZE(rust, c) \
	typedef char check_##rust##_size[sizeof(rust) == sizeof(c) ? 1 : -1]

#define CHECK_FIELD(rust, c, field) \
	typedef char check_##rust##_##field[ \
		(offsetof(rust, field) == offsetof(c, field) && \
		sizeof(((rust *)0)->field) == sizeof(((c *)0)->field)) ? 1 : -1]

fiftyoneDegreesResultsHash* fiftyoneDegreesResultsHashCreate(
	fiftyoneDegreesResourceManager *manager,
	uint32_t userAgentCapacity,
	uint32_t overridesCapacity);

/* fiftyone-common-sys */

typedef struct {
	const char *file;
	const char *func;
	int line;
	int status;
} RustException;
CHECK_SIZE(RustException, fiftyoneDegreesException);
CHECK_FIELD(RustException, fiftyoneDegreesException, file);
CHECK_FIELD(RustException, fiftyoneDegreesException, func);
CHECK_FIELD(RustException, fiftyoneDegreesException, line);
CHECK_FIELD(RustException, fiftyoneDegreesException, status);

typedef struct {
	void *active;
} RustResourceManager;
CHECK_SIZE(RustResourceManager, fiftyoneDegreesResourceManager);
CHECK_FIELD(RustResourceManager, fiftyoneDegreesResourceManager, active);

typedef struct {
	const char *prefix;
	size_t prefixLength;
	int prefixEnum;
} RustEvidencePrefixMap;
CHECK_SIZE(RustEvidencePrefixMap, fiftyoneDegreesEvidencePrefixMap);
CHECK_FIELD(RustEvidencePrefixMap, fiftyoneDegreesEvidencePrefixMap, prefix);
CHECK_FIELD(RustEvidencePrefixMap, fiftyoneDegreesEvidencePrefixMap, prefixLength);
CHECK_FIELD(RustEvidencePrefixMap, fiftyoneDegreesEvidencePrefixMap, prefixEnum);

typedef struct {
	const char *key;
	size_t keyLength;
	const char *value;
	size_t valueLength;
} RustKeyValuePair;
CHECK_SIZE(RustKeyValuePair, fiftyoneDegreesKeyValuePair);
CHECK_FIELD(RustKeyValuePair, fiftyoneDegreesKeyValuePair, key);
CHECK_FIELD(RustKeyValuePair, fiftyoneDegreesKeyValuePair, keyLength);
CHECK_FIELD(RustKeyValuePair, fiftyoneDegreesKeyValuePair, value);
CHECK_FIELD(RustKeyValuePair, fiftyoneDegreesKeyValuePair, valueLength);

typedef struct {
	int prefix;
	RustKeyValuePair item;
	const void *parsedValue;
	size_t parsedLength;
	void *header;
} RustEvidenceKeyValuePair;
CHECK_SIZE(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair);
CHECK_FIELD(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair, prefix);
CHECK_FIELD(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair, item);
CHECK_FIELD(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair, parsedValue);
CHECK_FIELD(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair, parsedLength);
CHECK_FIELD(RustEvidenceKeyValuePair, fiftyoneDegreesEvidenceKeyValuePair, header);

typedef struct {
	uint32_t count;
	uint32_t capacity;
	void *items;
	void *next;
	void *prev;
} RustEvidenceKeyValuePairArray;
CHECK_SIZE(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray);
CHECK_FIELD(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray, count);
CHECK_FIELD(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray, capacity);
CHECK_FIELD(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray, items);
CHECK_FIELD(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray, next);
CHECK_FIELD(RustEvidenceKeyValuePairArray, fiftyoneDegreesEvidenceKeyValuePairArray, prev);

typedef struct {
	const char **array;
	int count;
	const char *string;
	void *existing;
} RustPropertiesRequired;
CHECK_SIZE(RustPropertiesRequired, fiftyoneDegreesPropertiesRequired);
CHECK_FIELD(RustPropertiesRequired, fiftyoneDegreesPropertiesRequired, array);
CHECK_FIELD(RustPropertiesRequired, fiftyoneDegreesPropertiesRequired, count);
CHECK_FIELD(RustPropertiesRequired, fiftyoneDegreesPropertiesRequired, string);
CHECK_FIELD(RustPropertiesRequired, fiftyoneDegreesPropertiesRequired, existing);

/* src/lib.rs */

typedef struct {
	bool loaded;
	uint32_t capacity;
	uint16_t concurrency;
} RustCollectionConfig;
CHECK_SIZE(RustCollectionConfig, fiftyoneDegreesCollectionConfig);
CHECK_FIELD(RustCollectionConfig, fiftyoneDegreesCollectionConfig, loaded);
CHECK_FIELD(RustCollectionConfig, fiftyoneDegreesCollectionConfig, capacity);
CHECK_FIELD(RustCollectionConfig, fiftyoneDegreesCollectionConfig, concurrency);

typedef struct {
	bool allInMemory;
	bool usesUpperPrefixedHeaders;
	bool freeData;
	bool useTempFile;
	bool reuseTempFile;
	const char **tempDirs;
	int tempDirCount;
	bool propertyValueIndex;
} RustConfigBase;
CHECK_SIZE(RustConfigBase, fiftyoneDegreesConfigBase);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, allInMemory);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, usesUpperPrefixedHeaders);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, freeData);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, useTempFile);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, reuseTempFile);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, tempDirs);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, tempDirCount);
CHECK_FIELD(RustConfigBase, fiftyoneDegreesConfigBase, propertyValueIndex);

typedef struct {
	RustConfigBase b;
	bool updateMatchedUserAgent;
	size_t maxMatchedUserAgentLength;
	bool allowUnmatched;
	bool processSpecialEvidence;
} RustConfigDeviceDetection;
CHECK_SIZE(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection);
CHECK_FIELD(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection, b);
CHECK_FIELD(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection, updateMatchedUserAgent);
CHECK_FIELD(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection, maxMatchedUserAgentLength);
CHECK_FIELD(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection, allowUnmatched);
CHECK_FIELD(RustConfigDeviceDetection, fiftyoneDegreesConfigDeviceDetection, processSpecialEvidence);

typedef struct {
	RustConfigDeviceDetection b;
	RustCollectionConfig strings;
	RustCollectionConfig components;
	RustCollectionConfig maps;
	RustCollectionConfig properties;
	RustCollectionConfig values;
	RustCollectionConfig profiles;
	RustCollectionConfig rootNodes;
	RustCollectionConfig nodes;
	RustCollectionConfig profileOffsets;
	int32_t difference;
	int32_t drift;
	bool usePerformanceGraph;
	bool usePredictiveGraph;
	bool traceRoute;
} RustConfigHash;
CHECK_SIZE(RustConfigHash, fiftyoneDegreesConfigHash);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, b);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, strings);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, components);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, maps);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, properties);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, values);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, profiles);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, rootNodes);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, nodes);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, profileOffsets);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, difference);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, drift);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, usePerformanceGraph);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, usePredictiveGraph);
CHECK_FIELD(RustConfigHash, fiftyoneDegreesConfigHash, traceRoute);
