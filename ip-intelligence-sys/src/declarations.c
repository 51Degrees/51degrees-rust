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
 * Compiled by build.rs to check the C structures declared by hand in
 * src/lib.rs. Rust cannot see a C header, so a changed structure would still
 * build and then read the wrong memory. An array of negative size is an error
 * in C, and the macros below declare one when a structure stops matching, so
 * the build fails instead.
 *
 * Each structure here must match its Rust declaration, and uses the C field
 * names so that each field can be checked by name. A new C field that fits in
 * existing padding moves nothing, so these checks cannot see one.
 */

#include <stddef.h>

#include "ipi.h"
#include "ipi_weighted_results.h"

#define CHECK_SIZE(rust, c) \
	typedef char check_##rust##_size[sizeof(rust) == sizeof(c) ? 1 : -1]

#define CHECK_FIELD(rust, c, field) \
	typedef char check_##rust##_##field[ \
		(offsetof(rust, field) == offsetof(c, field) && \
		sizeof(((rust *)0)->field) == sizeof(((c *)0)->field)) ? 1 : -1]

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

typedef struct {
	uint8_t *ptr;
	uint32_t allocated;
	uint32_t used;
} RustData;
CHECK_SIZE(RustData, fiftyoneDegreesData);
CHECK_FIELD(RustData, fiftyoneDegreesData, ptr);
CHECK_FIELD(RustData, fiftyoneDegreesData, allocated);
CHECK_FIELD(RustData, fiftyoneDegreesData, used);

typedef struct {
	RustData data;
	void *handle;
	const void *collection;
} RustCollectionItem;
CHECK_SIZE(RustCollectionItem, fiftyoneDegreesCollectionItem);
CHECK_FIELD(RustCollectionItem, fiftyoneDegreesCollectionItem, data);
CHECK_FIELD(RustCollectionItem, fiftyoneDegreesCollectionItem, handle);
CHECK_FIELD(RustCollectionItem, fiftyoneDegreesCollectionItem, collection);

typedef struct {
	int32_t valueType;
	int requiredPropertyIndex;
	uint32_t rawWeighting;
} RustWeightedValueHeader;
CHECK_SIZE(RustWeightedValueHeader, fiftyoneDegreesWeightedValueHeader);
CHECK_FIELD(RustWeightedValueHeader, fiftyoneDegreesWeightedValueHeader, valueType);
CHECK_FIELD(RustWeightedValueHeader, fiftyoneDegreesWeightedValueHeader, requiredPropertyIndex);
CHECK_FIELD(RustWeightedValueHeader, fiftyoneDegreesWeightedValueHeader, rawWeighting);

typedef struct {
	RustWeightedValueHeader header;
	int32_t value;
} RustWeightedInt;
CHECK_SIZE(RustWeightedInt, fiftyoneDegreesWeightedInt);
CHECK_FIELD(RustWeightedInt, fiftyoneDegreesWeightedInt, header);
CHECK_FIELD(RustWeightedInt, fiftyoneDegreesWeightedInt, value);

typedef struct {
	RustWeightedValueHeader header;
	double value;
} RustWeightedDouble;
CHECK_SIZE(RustWeightedDouble, fiftyoneDegreesWeightedDouble);
CHECK_FIELD(RustWeightedDouble, fiftyoneDegreesWeightedDouble, header);
CHECK_FIELD(RustWeightedDouble, fiftyoneDegreesWeightedDouble, value);

typedef struct {
	RustWeightedValueHeader header;
	bool value;
} RustWeightedBool;
CHECK_SIZE(RustWeightedBool, fiftyoneDegreesWeightedBool);
CHECK_FIELD(RustWeightedBool, fiftyoneDegreesWeightedBool, header);
CHECK_FIELD(RustWeightedBool, fiftyoneDegreesWeightedBool, value);

typedef struct {
	RustWeightedValueHeader header;
	uint8_t value;
} RustWeightedByte;
CHECK_SIZE(RustWeightedByte, fiftyoneDegreesWeightedByte);
CHECK_FIELD(RustWeightedByte, fiftyoneDegreesWeightedByte, header);
CHECK_FIELD(RustWeightedByte, fiftyoneDegreesWeightedByte, value);

typedef struct {
	RustWeightedValueHeader header;
	RustData stringData;
	const char *value;
} RustWeightedString;
CHECK_SIZE(RustWeightedString, fiftyoneDegreesWeightedString);
CHECK_FIELD(RustWeightedString, fiftyoneDegreesWeightedString, header);
CHECK_FIELD(RustWeightedString, fiftyoneDegreesWeightedString, stringData);
CHECK_FIELD(RustWeightedString, fiftyoneDegreesWeightedString, value);

typedef struct {
	RustData valuesData;
	RustData itemsData;
	RustWeightedValueHeader **items;
	uint32_t itemsCount;
} RustWeightedValuesCollection;
CHECK_SIZE(RustWeightedValuesCollection, fiftyoneDegreesWeightedValuesCollection);
CHECK_FIELD(RustWeightedValuesCollection, fiftyoneDegreesWeightedValuesCollection, valuesData);
CHECK_FIELD(RustWeightedValuesCollection, fiftyoneDegreesWeightedValuesCollection, itemsData);
CHECK_FIELD(RustWeightedValuesCollection, fiftyoneDegreesWeightedValuesCollection, items);
CHECK_FIELD(RustWeightedValuesCollection, fiftyoneDegreesWeightedValuesCollection, itemsCount);

typedef struct {
	RustCollectionItem item;
	uint32_t rawWeighting;
} RustProfilePercentage;
CHECK_SIZE(RustProfilePercentage, fiftyoneDegreesProfilePercentage);
CHECK_FIELD(RustProfilePercentage, fiftyoneDegreesProfilePercentage, item);
CHECK_FIELD(RustProfilePercentage, fiftyoneDegreesProfilePercentage, rawWeighting);
