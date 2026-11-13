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
 * Property enumeration shim for fiftyone-device-detection-sys.
 *
 * The set of properties a Hash data set was initialized with lives in the
 * `available` member of the base data set, several layers of nested structures
 * deep. Reaching it from Rust would require mirroring every intermediate C
 * struct exactly, which is fragile across platforms and compiler versions. The
 * helpers below read those private layouts from C, where the headers describe
 * them, and present a flat C ABI to the Rust crate. They use only the public
 * Hash and common C library headers, so they stay correct as the upstream
 * layouts change.
 */

#include <string.h>

#include "hash/hash.h"

/*
 * Copies `value` into `buffer` as a null terminated string, writing at most
 * `length - 1` characters before the terminator, and returns the number of
 * characters written excluding the terminator. Writes nothing and returns zero
 * when `value` or `buffer` is null or `length` is zero.
 */
static size_t copy_bounded(char *buffer, size_t length, const char *value) {
    size_t written = 0;
    if (value != NULL && buffer != NULL && length > 0) {
        size_t i = 0;
        while (value[i] != '\0' && i < length - 1) {
            buffer[i] = value[i];
            i++;
        }
        buffer[i] = '\0';
        written = i;
    }
    return written;
}

/*
 * Returns the number of required property indexes of the data set managed by
 * `manager`, which is the number of names it was created with, or when it was
 * created naming no property, the number of its available properties. Returns
 * zero if the data set or its available properties are not present. The data
 * set reference is taken and released around the read.
 */
uint32_t fiftyoneDegreesShimHashGetRequiredPropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = fiftyoneDegreesPropertiesGetRequiredPropertyCount(
        dataSet->b.b.available);
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Writes the name of the required property at `requiredPropertyIndex` into
 * `buffer` as a null terminated string and returns the number of characters
 * written, excluding the terminator. A name the data set has no property for
 * is written as it was given. Returns zero when the index is out of range, the
 * buffer is too small or no data set is available. The buffer is always null
 * terminated when `length` is at least one.
 */
size_t fiftyoneDegreesShimHashGetRequiredPropertyName(
    fiftyoneDegreesResourceManager *manager,
    int requiredPropertyIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }

    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }

    size_t written = 0;
    fiftyoneDegreesPropertiesAvailable *available = dataSet->b.b.available;
    if (available != NULL &&
        requiredPropertyIndex >= 0 &&
        (uint32_t)requiredPropertyIndex <
            fiftyoneDegreesPropertiesGetRequiredPropertyCount(available)) {
        fiftyoneDegreesString *name =
            fiftyoneDegreesPropertiesGetNameFromRequiredIndex(
                available, requiredPropertyIndex);
        const char *value = FIFTYONE_DEGREES_STRING(name);
        written = copy_bounded(buffer, length, value);
    }

    fiftyoneDegreesDataSetHashRelease(dataSet);
    return written;
}

/*
 * Returns the zero based required property index for the property named
 * `propertyName`, or -1 when the property is not one of the required
 * properties. This index is the value expected by the results getters such as
 * fiftyoneDegreesResultsHashGetHasValues.
 */
int fiftyoneDegreesShimHashGetRequiredPropertyIndexFromName(
    fiftyoneDegreesResourceManager *manager,
    const char *propertyName) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return -1;
    }
    int index = -1;
    if (dataSet->b.b.available != NULL) {
        index = fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
            dataSet->b.b.available, propertyName);
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return index;
}

/*
 * Returns the required property index of the property named `propertyName`
 * in `dataSet`, found by the C library ignoring case, or -1 when the property
 * is not one of the required properties. It takes no reference on the data
 * set, so the caller holds one for as long as it calls this, and it only
 * reads properties that do not change while that reference is held.
 */
int fiftyoneDegreesShimHashDataSetGetRequiredPropertyIndexFromName(
    fiftyoneDegreesDataSetHash *dataSet,
    const char *propertyName) {
    if (dataSet == NULL || dataSet->b.b.available == NULL) {
        return -1;
    }
    return fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
        dataSet->b.b.available, propertyName);
}

/*
 * Returns the data set `results` hold a reference to, which is released when
 * the results are freed. It takes no reference of its own.
 */
fiftyoneDegreesDataSetHash* fiftyoneDegreesShimHashResultsGetDataSet(
    fiftyoneDegreesResultsHash *results) {
    return (fiftyoneDegreesDataSetHash*)results->b.b.dataSet;
}

/*
 * Reloads the data set `manager` holds from a copy of the `length` bytes at
 * `data`, as the C++ engine's refreshData does. The copy is made with the C
 * library's allocator, and the configuration of the data set in use, which
 * the reload copies, is set to free it, so the new data set frees the copy
 * when it is freed. The flag is read only while a data set is initialised.
 * When the C library cannot allocate the new data set itself the copy is not
 * freed, because the status does not tell that failure apart from the ones
 * after which the C library has freed it.
 *
 * The flag stays on for the life of the manager, because every reload copies
 * the configuration of the data set in use. So after a reload through this
 * function, a reload through the C library's own reload from memory frees the
 * bytes it is given, which a caller that still owns them must not allow.
 */
fiftyoneDegreesStatusCode fiftyoneDegreesShimHashReloadManagerFromMemoryCopy(
    fiftyoneDegreesResourceManager *manager,
    const void *data,
    long length,
    fiftyoneDegreesException *exception) {
    if (data == NULL) {
        return FIFTYONE_DEGREES_STATUS_NULL_POINTER;
    }
    if (length <= 0) {
        return FIFTYONE_DEGREES_STATUS_CORRUPT_DATA;
    }
    void *copy = fiftyoneDegreesMalloc((size_t)length);
    if (copy == NULL) {
        return FIFTYONE_DEGREES_STATUS_INSUFFICIENT_MEMORY;
    }
    memcpy(copy, data, (size_t)length);
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    ((fiftyoneDegreesConfigHash*)&dataSet->config)->b.b.freeData = true;
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return fiftyoneDegreesHashReloadManagerFromMemory(
        manager,
        copy,
        length,
        exception);
}

/*
 * Writes the data set's name into `buffer` as a null terminated string and
 * returns the number of characters written, excluding the terminator. The name
 * is the data file's tier (for example "Lite", "Enterprise" or "TAC"), read
 * from the data file header's name offset in the strings collection. Returns
 * zero when the name cannot be read or no data set is available. The buffer is
 * always null terminated when `length` is at least one.
 */
size_t fiftyoneDegreesShimHashGetDataSetName(
    fiftyoneDegreesResourceManager *manager,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }

    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }

    size_t written = 0;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesString *name = fiftyoneDegreesStringGet(
        dataSet->strings, (uint32_t)dataSet->header.nameOffset, &item, exception);
    if (name != NULL) {
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            written = copy_bounded(buffer, length, FIFTYONE_DEGREES_STRING(name));
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }

    fiftyoneDegreesDataSetHashRelease(dataSet);
    return written;
}

/*
 * Returns the size of a Hash data file's header in bytes, being how much of a
 * data file the C library compares to find a copy of it to reuse.
 */
size_t fiftyoneDegreesShimHashGetHeaderSize(void) {
    return sizeof(fiftyoneDegreesDataSetHashHeader);
}

/*
 * Returns the number of results, one for each component, or zero when
 * `results` is NULL.
 */
uint32_t fiftyoneDegreesShimHashGetResultCount(
    fiftyoneDegreesResultsHash *results) {
    return results == NULL ? 0 : results->count;
}

/*
 * Returns the number of components whose graph the results have still to
 * walk, or zero when `results` is NULL. When the data set was created with no
 * properties named, populating the results walks no graph, and the first read
 * of one of a component's properties walks that component's graph.
 */
uint32_t fiftyoneDegreesShimHashGetPendingGraphCount(
    fiftyoneDegreesResultsHash *results) {
    uint32_t count = 0;
    if (results == NULL) {
        return 0;
    }
    for (uint32_t word = 0; word < FIFTYONE_DEGREES_RESULTS_PENDING_WORDS;
        word++) {
        uint32_t bits = results->b.b.pending[word];
        while (bits != 0) {
            bits &= bits - 1;
            count++;
        }
    }
    return count;
}

/*
 * Returns the number of handles to the data file that the data set managed
 * by `manager` opened when it loaded, which stay open until the data set is
 * freed. Every configuration that does not load the whole file into memory
 * opens one for each concurrent operation it expects, even when all of its
 * collections are then held in memory and no read uses the handles.
 */
uint16_t fiftyoneDegreesShimHashGetFilePoolSize(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    const uint16_t count = dataSet->b.b.filePool.pool.count;
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Returns the size of fiftyoneDegreesConfigHash, for a test that the Rust
 * declaration of the structure has the same size.
 */
size_t fiftyoneDegreesShimHashGetConfigSize(void) {
    return sizeof(fiftyoneDegreesConfigHash);
}

/*
 * Sets `strings` and `values` to the type names of the strings and values
 * collections of the data set managed by `manager`, and returns 1, or returns
 * 0 when there is no data set. "CollectionMemory" holds the whole collection
 * in memory, and "CollectionHeld" holds only what the required properties
 * read. The names are constants of the C library, so they outlive the data
 * set.
 */
int fiftyoneDegreesShimHashGetCollectionTypeNames(
    fiftyoneDegreesResourceManager *manager,
    const char **strings,
    const char **values) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    *strings = dataSet->strings->typeName;
    *values = dataSet->values->typeName;
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return 1;
}

/*
 * The metadata readers below follow the C++ builders in 51degrees-cxx
 * (src/dd/hash/PropertyMetaDataBuilderHash.cpp,
 * ComponentMetaDataBuilderHash.cpp and ValueMetaDataBuilderHash.cpp), so the
 * crates describe a data file as the other SDKs do. Each string reader writes
 * as much of the string as fits, null terminated, and returns the string's
 * full length excluding the terminator, so a caller whose buffer was too small
 * can call again with one of that length plus one. A string that is absent is
 * written as empty and its length is zero. When the string, or the property,
 * value or component it belongs to, cannot be read, or an index is out of
 * range, the buffer is left empty and SHIM_STRING_FAILED is returned, as the
 * C++ builders raise an exception there rather than leave something out. The
 * property values reader instead gives each string to a callback, because a
 * property can have over a hundred thousand values.
 */

/* The string fields of a data file property. */
#define SHIM_PROPERTY_NAME 0
#define SHIM_PROPERTY_DESCRIPTION 1
#define SHIM_PROPERTY_CATEGORY 2
#define SHIM_PROPERTY_URL 3

/* The strings of a property value, in the order they are given. */
#define SHIM_VALUE_NAME 0
#define SHIM_VALUE_DESCRIPTION 1
#define SHIM_VALUE_URL 2

/*
 * Takes one string of a property value, `field` being one of the
 * SHIM_VALUE_ constants, from fiftyoneDegreesShimHashGetDataFilePropertyValues
 * or fiftyoneDegreesShimHashGetResultValues.
 */
typedef void (*fiftyoneDegreesShimValueStringCallback)(
    void *context,
    int field,
    const char *value);

/* Returned by a string reader that could not read the string. */
#define SHIM_STRING_FAILED ((size_t)-1)

/* What fiftyoneDegreesShimHashFindDataFilePropertyValue returns. */
#define SHIM_FIND_FAILED 0
#define SHIM_FIND_FOUND 1
#define SHIM_FIND_NOT_FOUND 2

/* What fiftyoneDegreesShimHashGetResultValues returns. */
#define SHIM_VALUES_FAILED 0
#define SHIM_VALUES_READ 1
#define SHIM_VALUES_NONE 2

/*
 * Writes as much of `value` as fits into `buffer` and returns the full length
 * of `value`, or writes an empty string and returns zero when `value` is null.
 */
static size_t copy_measured(char *buffer, size_t length, const char *value) {
    if (value == NULL) {
        if (buffer != NULL && length > 0) {
            buffer[0] = '\0';
        }
        return 0;
    }
    copy_bounded(buffer, length, value);
    return strlen(value);
}

/*
 * Writes the string at `offset` in the strings collection into `buffer` and
 * returns its full length, or SHIM_STRING_FAILED when it cannot be read.
 */
static size_t copy_string_at(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t offset,
    char *buffer,
    size_t length) {
    size_t total = SHIM_STRING_FAILED;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesString *value = fiftyoneDegreesStringGet(
        dataSet->strings, offset, &item, exception);
    if (value != NULL) {
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            total = copy_measured(
                buffer, length, FIFTYONE_DEGREES_STRING(value));
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }
    return total;
}

/*
 * Gets the data file property at `propertyIndex` into `item` and returns it,
 * or returns NULL when the index is out of range or the property cannot be
 * read. The caller releases `item` from the properties collection when the
 * result is not NULL.
 */
static const fiftyoneDegreesProperty* get_property(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t propertyIndex,
    fiftyoneDegreesCollectionItem *item) {
    if (propertyIndex >=
        fiftyoneDegreesCollectionGetCount(dataSet->properties)) {
        return NULL;
    }
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesDataReset(&item->data);
    const fiftyoneDegreesProperty *property = fiftyoneDegreesPropertyGet(
        dataSet->properties, propertyIndex, item, exception);
    if (property != NULL && FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, item);
        return NULL;
    }
    return property;
}

/*
 * Finds `property` by name among the available properties, as the C++
 * builder does, and sets `available` to 1 when the data set was created with
 * it, being when it has a required property index, otherwise 0, and
 * `requiredIndex` to that index, which the results getters take, or -1 when
 * it has none. The C library matches the name ignoring case, so a property
 * named in another case is found. Returns 1, or 0 when the name cannot be
 * read, as the C++ builder raises an exception there.
 */
static int find_available(
    fiftyoneDegreesDataSetHash *dataSet,
    const fiftyoneDegreesProperty *property,
    uint8_t *available,
    int32_t *requiredIndex) {
    *available = 0;
    *requiredIndex = -1;
    if (dataSet->b.b.available == NULL) {
        return 1;
    }
    int read = 0;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesString *name = fiftyoneDegreesStringGet(
        dataSet->strings, property->nameOffset, &item, exception);
    if (name != NULL) {
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            const char *value = FIFTYONE_DEGREES_STRING(name);
            // Available when results can hold its values, which needs a
            // required index. A data set reloaded with the names of the one
            // before holds a property only it has with no required index.
            *requiredIndex =
                fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
                    dataSet->b.b.available, value);
            *available = *requiredIndex >= 0 ? 1 : 0;
            read = 1;
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }
    return read;
}

/*
 * Returns the number of values the data file holds for `property`, or zero
 * when they are not all in the values collection, as when both value indexes
 * are -1. The C library's profile index counts them the same way.
 */
static uint32_t value_count(
    fiftyoneDegreesDataSetHash *dataSet,
    const fiftyoneDegreesProperty *property) {
    const uint32_t first = property->firstValueIndex;
    const uint32_t last = property->lastValueIndex;
    return first <= last &&
        last < fiftyoneDegreesCollectionGetCount(dataSet->values) ?
        last - first + 1 : 0;
}

/*
 * Returns the component of the data set at `componentIndex` in its list of
 * components, or NULL when the index is out of range.
 */
static fiftyoneDegreesComponent* get_component(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t componentIndex) {
    if (componentIndex >= dataSet->componentsList.count) {
        return NULL;
    }
    return (fiftyoneDegreesComponent*)
        dataSet->componentsList.items[componentIndex].data.ptr;
}

/*
 * Returns a copy of the name of the value at `valueIndex` in the values
 * collection, which the caller frees with fiftyoneDegreesFree, or NULL when
 * the value or its name cannot be read or the copy cannot be made. The value
 * is released before its name is read, so one item is held at a time.
 */
static char* copy_value_name(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t valueIndex) {
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesValue *value = fiftyoneDegreesValueGet(
        dataSet->values, valueIndex, &item, exception);
    if (value == NULL) {
        return NULL;
    }
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
        return NULL;
    }
    const uint32_t nameOffset = (uint32_t)value->nameOffset;
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
    const fiftyoneDegreesString *name = fiftyoneDegreesStringGet(
        dataSet->strings, nameOffset, &item, exception);
    if (name == NULL) {
        return NULL;
    }
    char *copy = NULL;
    if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
        const char *text = FIFTYONE_DEGREES_STRING(name);
        const size_t length = strlen(text) + 1;
        copy = (char*)fiftyoneDegreesMalloc(length);
        if (copy != NULL) {
            memcpy(copy, text, length);
        }
    }
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    return copy;
}

/*
 * Returns 1 when the value at `valueIndex` in the values collection is one of
 * `property`'s and is named `name`, or 0 when it is not or cannot be read.
 */
static int is_value_named(
    fiftyoneDegreesDataSetHash *dataSet,
    const fiftyoneDegreesProperty *property,
    uint32_t valueIndex,
    const char *name) {
    if (valueIndex < property->firstValueIndex ||
        valueIndex > property->lastValueIndex) {
        return 0;
    }
    char *found = copy_value_name(dataSet, valueIndex);
    if (found == NULL) {
        return 0;
    }
    const int named = strcmp(found, name) == 0;
    fiftyoneDegreesFree(found);
    return named;
}

/*
 * Sets `index` to the index in the values collection of `property`'s value
 * named `name`, or to -1 when it has none, and returns 1, or returns 0 when a
 * value or a string cannot be read. The name must match exactly. A
 * property's values are in order of their names, so the C library's binary
 * search finds it, as the C++ ValueMetaDataCollectionBaseHash::getByKey does,
 * without reading the others. A value the data file holds out of that order
 * can be missed, as the C++ misses it.
 */
static int find_value(
    fiftyoneDegreesDataSetHash *dataSet,
    const fiftyoneDegreesProperty *property,
    const char *name,
    long *index) {
    *index = -1;
    // The search can read any place between the two value indexes, so it
    // runs only when value_count finds them all in the values collection.
    if (value_count(dataSet, property) == 0) {
        return 1;
    }
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    *index = fiftyoneDegreesValueGetIndexByNameAndType(
        dataSet->values,
        dataSet->strings,
        property,
        FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_STRING,
        name,
        exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        *index = -1;
        return 0;
    }
    // When the search reads nothing for a value and sets no exception, it
    // answers as though that value matched, so the value found is compared
    // again.
    return *index < 0 ||
        is_value_named(dataSet, property, (uint32_t)*index, name);
}

/*
 * Sets `number` to the place of `property`'s default value among its values,
 * or to -1 when it has none, and returns 1, or returns 0 when a value or a
 * string cannot be read. As the C++ MetaDataHash::getDefaultValueForProperty
 * finds it, the default is the property's own value with the name
 * fiftyoneDegreesValueGetDefaultText gives, being the name of the value at
 * the default value index, or an empty name when there is no index. So a
 * property with no index can have a default, and one whose index names none
 * of its values has none.
 */
static int default_value_number(
    fiftyoneDegreesDataSetHash *dataSet,
    const fiftyoneDegreesProperty *property,
    int32_t *number) {
    *number = -1;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    // No property types, as a Hash data set stores every value as a string,
    // and no buffer, so the name is always in memory freed below.
    char *name = fiftyoneDegreesValueGetDefaultText(
        dataSet->values,
        dataSet->strings,
        NULL,
        property,
        NULL,
        0,
        exception);
    if (name == NULL) {
        return 0;
    }
    long index = -1;
    const int found = find_value(dataSet, property, name, &index);
    fiftyoneDegreesFree(name);
    if (found == 0) {
        return 0;
    }
    if (index >= 0) {
        *number = (int32_t)((uint32_t)index - property->firstValueIndex);
    }
    return 1;
}

/*
 * Returns the number of properties the data file defines, including those the
 * data set was not created with, or zero when no data set is available.
 */
uint32_t fiftyoneDegreesShimHashGetDataFilePropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = fiftyoneDegreesCollectionGetCount(dataSet->properties);
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Reads the fields of the data file property at `propertyIndex` that are not
 * strings into the output parameters and returns 1, or returns 0 when the
 * index is out of range or the property, its name or its default value cannot
 * be read. The name is read to find whether the data set was created with the
 * property.
 * `valueType` is the fiftyoneDegreesPropertyValueType the property's values
 * have. `available` is 1 when the data set was created with the property, so
 * results can hold its values, and `requiredIndex` is the index the results
 * getters take for it, or -1 when it has none. `componentId` is the id of the
 * component the property belongs to, or 0 when the property names no
 * component.
 * `valueCount` is the number of values the data file holds for the property,
 * `defaultValueNumber` the place of its default value among them, found as
 * default_value_number finds it, or -1 when it has none, and `mapCount` the
 * number of data tiers it is in.
 */
int fiftyoneDegreesShimHashGetDataFileProperty(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    uint8_t *valueType,
    uint8_t *available,
    int32_t *requiredIndex,
    uint8_t *componentId,
    uint8_t *displayOrder,
    uint8_t *isMandatory,
    uint8_t *isList,
    uint8_t *isObsolete,
    uint8_t *show,
    uint8_t *showValues,
    uint32_t *valueCount,
    int32_t *defaultValueNumber,
    uint32_t *mapCount) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *property =
        get_property(dataSet, propertyIndex, &item);
    if (property != NULL) {
        const fiftyoneDegreesComponent *component =
            get_component(dataSet, property->componentIndex);
        *valueType = property->valueType;
        result = find_available(dataSet, property, available, requiredIndex);
        *componentId = component == NULL ? 0 : component->componentId;
        *displayOrder = property->displayOrder;
        *isMandatory = property->isMandatory;
        *isList = property->isList;
        *isObsolete = property->isObsolete;
        *show = property->show;
        *showValues = property->showValues;
        *valueCount = value_count(dataSet, property);
        *mapCount = property->mapCount;
        // A copy, so the property is released before its default value is
        // found.
        const fiftyoneDegreesProperty copy = *property;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
        result = result == 1 &&
            default_value_number(dataSet, &copy, defaultValueNumber) == 1;
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return result;
}

/*
 * Writes the string field `field` of the data file property at
 * `propertyIndex` into `buffer` and returns its full length. The fields are
 * SHIM_PROPERTY_NAME, SHIM_PROPERTY_DESCRIPTION, SHIM_PROPERTY_CATEGORY and
 * SHIM_PROPERTY_URL. A property with no description, category or URL has an
 * empty one. Returns SHIM_STRING_FAILED when the property or the string
 * cannot be read, or `field` is none of these.
 */
size_t fiftyoneDegreesShimHashGetDataFilePropertyString(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    int field,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return SHIM_STRING_FAILED;
    }
    size_t total = SHIM_STRING_FAILED;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *property =
        get_property(dataSet, propertyIndex, &item);
    if (property != NULL) {
        if (field >= SHIM_PROPERTY_NAME && field <= SHIM_PROPERTY_URL) {
            total = 0;
        }
        switch (field) {
        case SHIM_PROPERTY_NAME:
            total = copy_string_at(
                dataSet, property->nameOffset, buffer, length);
            break;
        case SHIM_PROPERTY_DESCRIPTION:
            if ((int32_t)property->descriptionOffset != -1) {
                total = copy_string_at(
                    dataSet, property->descriptionOffset, buffer, length);
            }
            break;
        case SHIM_PROPERTY_CATEGORY:
            if ((int32_t)property->categoryOffset != -1) {
                total = copy_string_at(
                    dataSet, property->categoryOffset, buffer, length);
            }
            break;
        case SHIM_PROPERTY_URL:
            // A property with no URL points at offset 0, which holds the
            // first string in the file rather than a URL.
            if ((int32_t)property->urlOffset > 0) {
                total = copy_string_at(
                    dataSet, property->urlOffset, buffer, length);
            }
            break;
        default:
            break;
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return total;
}

/*
 * Writes the name of the data tier at `mapIndex` among those the data file
 * property at `propertyIndex` is in into `buffer`, and returns its full
 * length, or SHIM_STRING_FAILED when either index is out of range or the
 * name cannot be read.
 */
size_t fiftyoneDegreesShimHashGetDataFilePropertyMap(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    uint32_t mapIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return SHIM_STRING_FAILED;
    }
    size_t total = SHIM_STRING_FAILED;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *property =
        get_property(dataSet, propertyIndex, &item);
    if (property != NULL) {
        if (mapIndex < property->mapCount) {
            FIFTYONE_DEGREES_EXCEPTION_CREATE;
            const int32_t offset = fiftyoneDegreesCollectionGetInteger32(
                dataSet->maps,
                property->firstMapIndex + mapIndex,
                exception);
            if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
                total = copy_string_at(
                    dataSet, (uint32_t)offset, buffer, length);
            }
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return total;
}

/*
 * Gives `callback` the string at `offset` in the strings collection as
 * `field` and returns 1, or returns 0 when the string cannot be read.
 */
static int give_string_at(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t offset,
    int field,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    int result = 0;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesString *value = fiftyoneDegreesStringGet(
        dataSet->strings, offset, &item, exception);
    if (value != NULL) {
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            callback(context, field, FIFTYONE_DEGREES_STRING(value));
            result = 1;
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }
    return result;
}

/*
 * Gives `callback` the name, description and URL of the value at
 * `valueIndex` in the values collection and returns 1, or returns 0 when the
 * value or one of its strings cannot be read. The value is released before
 * its strings are read, so one item is held at a time.
 */
static int give_value(
    fiftyoneDegreesDataSetHash *dataSet,
    uint32_t valueIndex,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesValue *value = fiftyoneDegreesValueGet(
        dataSet->values, valueIndex, &item, exception);
    if (value == NULL) {
        return 0;
    }
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
        return 0;
    }
    const uint32_t nameOffset = (uint32_t)value->nameOffset;
    const int32_t descriptionOffset = value->descriptionOffset;
    const int32_t urlOffset = value->urlOffsetOrWeight;
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
    if (give_string_at(
        dataSet, nameOffset, SHIM_VALUE_NAME, callback, context) == 0) {
        return 0;
    }
    // A value with no description points at offset 0, which holds the first
    // string in the file. A weighted value's URL offset holds its weight,
    // which is negative as a signed offset.
    if (descriptionOffset > 0) {
        if (give_string_at(
            dataSet,
            (uint32_t)descriptionOffset,
            SHIM_VALUE_DESCRIPTION,
            callback,
            context) == 0) {
            return 0;
        }
    }
    else {
        callback(context, SHIM_VALUE_DESCRIPTION, "");
    }
    if (urlOffset > 0) {
        if (give_string_at(
            dataSet,
            (uint32_t)urlOffset,
            SHIM_VALUE_URL,
            callback,
            context) == 0) {
            return 0;
        }
    }
    else {
        callback(context, SHIM_VALUE_URL, "");
    }
    return 1;
}

/*
 * Reads at most `count` of the values the data file holds for the property at
 * `propertyIndex`, from the one at `valueNumber` among them, in the data
 * file's order, and returns 1. For each value it calls `callback` with
 * `context` three times, being SHIM_VALUE_NAME with the name, then
 * SHIM_VALUE_DESCRIPTION with the description, then SHIM_VALUE_URL with the
 * URL, each a null terminated string valid only for that call. A value with
 * no description or URL gives an empty one. A `valueNumber` at or past the
 * last value reads none. The data set and the property are read once for the
 * whole run, and each value and string once. Returns 0 when the property
 * index is out of range or the property, a value or a string cannot be read,
 * after giving `callback` the strings read before the failure.
 */
int fiftyoneDegreesShimHashGetDataFilePropertyValues(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    uint32_t valueNumber,
    uint32_t count,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *property =
        get_property(dataSet, propertyIndex, &item);
    if (property != NULL) {
        const uint32_t first = property->firstValueIndex;
        const uint32_t total = value_count(dataSet, property);
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
        uint32_t end = valueNumber;
        if (valueNumber < total) {
            end = count < total - valueNumber ? valueNumber + count : total;
        }
        result = 1;
        for (uint32_t number = valueNumber; result == 1 && number < end;
            number++) {
            result = give_value(dataSet, first + number, callback, context);
        }
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return result;
}

/*
 * Finds the value named `name` among those the data file holds for the
 * property at `propertyIndex` and gives `callback` its strings as
 * fiftyoneDegreesShimHashGetDataFilePropertyValues gives each value. The value
 * is found as find_value finds it, as the C++
 * ValueMetaDataCollectionForPropertyHash::getByKey does. Returns
 * SHIM_FIND_FOUND, SHIM_FIND_NOT_FOUND when the property has no value with
 * that name, or SHIM_FIND_FAILED when the property index is out of range or
 * the property, a value or a string cannot be read.
 */
int fiftyoneDegreesShimHashFindDataFilePropertyValue(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    const char *name,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return SHIM_FIND_FAILED;
    }
    int result = SHIM_FIND_FAILED;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *held =
        get_property(dataSet, propertyIndex, &item);
    if (held != NULL) {
        // A copy, so the property is released before the search starts.
        const fiftyoneDegreesProperty property = *held;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
        long index = -1;
        if (find_value(dataSet, &property, name, &index) == 0) {
            result = SHIM_FIND_FAILED;
        }
        else if (index < 0) {
            result = SHIM_FIND_NOT_FOUND;
        }
        else {
            result = give_value(
                dataSet, (uint32_t)index, callback, context) == 1 ?
                SHIM_FIND_FOUND : SHIM_FIND_FAILED;
        }
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return result;
}

/*
 * Returns the number of properties the data set indexes the profiles of by
 * value, being the distinct names given when it was created, including any
 * the data file has no property of. Returns zero when none were named, as the
 * data set then has no index, or no data set is available.
 */
uint32_t fiftyoneDegreesShimHashGetIndexedPropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = dataSet->valueProfiles == NULL ?
        0 : dataSet->valueProfiles->propertiesCount;
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Writes the name at `nameIndex` among those the data set indexes, in the
 * order given, into `buffer` and returns its full length, or
 * SHIM_STRING_FAILED when the index is out of range or no data set is
 * available.
 */
size_t fiftyoneDegreesShimHashGetIndexedPropertyName(
    fiftyoneDegreesResourceManager *manager,
    uint32_t nameIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return SHIM_STRING_FAILED;
    }
    size_t total = SHIM_STRING_FAILED;
    const fiftyoneDegreesHashValueProfiles *index = dataSet->valueProfiles;
    if (index != NULL && nameIndex < index->propertiesCount) {
        total = copy_measured(buffer, length, index->names[nameIndex]);
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return total;
}

/*
 * Gives `callback` each value the results hold for the property at
 * `requiredPropertyIndex`, in order, with SHIM_VALUE_NAME and the value as a
 * null terminated string valid only for that call, and returns
 * SHIM_VALUES_READ. These are the values the C++
 * ResultsHash::getValuesInternal reads, and the matched profile can hold
 * none. A value with no string is left out, as the C++ leaves it out. Returns
 * SHIM_VALUES_NONE, giving nothing, when
 * fiftyoneDegreesResultsHashGetHasValues finds the results hold no value for
 * the property, which the C++ ResultsBase checks before reading any. Returns
 * SHIM_VALUES_FAILED when the values cannot be read, with `exception` set
 * when the C library set it.
 */
int fiftyoneDegreesShimHashGetResultValues(
    fiftyoneDegreesResultsHash *results,
    int requiredPropertyIndex,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context,
    fiftyoneDegreesException *exception) {
    const bool hasValues = fiftyoneDegreesResultsHashGetHasValues(
        results, requiredPropertyIndex, exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return SHIM_VALUES_FAILED;
    }
    if (hasValues == false) {
        return SHIM_VALUES_NONE;
    }
    const fiftyoneDegreesCollectionItem *values =
        fiftyoneDegreesResultsHashGetValues(
            results, requiredPropertyIndex, exception);
    if (values == NULL || FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return SHIM_VALUES_FAILED;
    }
    for (uint32_t i = 0; i < results->values.count; i++) {
        const fiftyoneDegreesString *value =
            (const fiftyoneDegreesString *)values[i].data.ptr;
        if (value != NULL) {
            callback(
                context,
                SHIM_VALUE_NAME,
                FIFTYONE_DEGREES_STRING(value));
        }
    }
    return SHIM_VALUES_READ;
}

/*
 * Returns the number of components the data set lists, or zero when no data
 * set is available.
 */
uint32_t fiftyoneDegreesShimHashGetComponentCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = dataSet->componentsList.count;
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Reads the id of the component at `componentIndex` and the profile id of its
 * default profile into the output parameters and returns 1, or returns 0 when
 * the index is out of range or the default profile cannot be read. A default
 * profile that is absent without a failure gives 1 with the id 0.
 */
int fiftyoneDegreesShimHashGetComponent(
    fiftyoneDegreesResourceManager *manager,
    uint32_t componentIndex,
    uint8_t *componentId,
    uint32_t *defaultProfileId) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    fiftyoneDegreesComponent *component =
        get_component(dataSet, componentIndex);
    if (component != NULL) {
        FIFTYONE_DEGREES_EXCEPTION_CREATE;
        const uint32_t profileId = fiftyoneDegreesComponentGetDefaultProfileId(
            dataSet->profiles, component, exception);
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            *componentId = component->componentId;
            *defaultProfileId = profileId;
            result = 1;
        }
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return result;
}

/*
 * Writes the name of the component at `componentIndex` into `buffer` and
 * returns its full length, or SHIM_STRING_FAILED when the index is out of
 * range or the name cannot be read.
 */
size_t fiftyoneDegreesShimHashGetComponentName(
    fiftyoneDegreesResourceManager *manager,
    uint32_t componentIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return SHIM_STRING_FAILED;
    }
    size_t total = SHIM_STRING_FAILED;
    const fiftyoneDegreesComponent *component =
        get_component(dataSet, componentIndex);
    if (component != NULL) {
        total = copy_string_at(
            dataSet, (uint32_t)component->nameOffset, buffer, length);
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return total;
}

/*
 * Returns the number of profiles the data file holds, or zero when no data
 * set is available.
 */
uint32_t fiftyoneDegreesShimHashGetProfileCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = fiftyoneDegreesCollectionGetCount(dataSet->profileOffsets);
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Reads the id of the profile at `profileIndex` among the data file's
 * profiles, which are in order of profile id, and the id of the component it
 * belongs to into the output parameters and returns 1, as the C++
 * ProfileMetaDataCollectionHash reads them. Returns 0 when the index is out
 * of range or the profile or its component cannot be read.
 */
int fiftyoneDegreesShimHashGetProfile(
    fiftyoneDegreesResourceManager *manager,
    uint32_t profileIndex,
    uint32_t *profileId,
    uint8_t *componentId) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    if (profileIndex <
        fiftyoneDegreesCollectionGetCount(dataSet->profileOffsets)) {
        FIFTYONE_DEGREES_EXCEPTION_CREATE;
        fiftyoneDegreesCollectionItem item;
        fiftyoneDegreesDataReset(&item.data);
        const fiftyoneDegreesProfile *profile =
            fiftyoneDegreesProfileGetByIndex(
                dataSet->profileOffsets,
                dataSet->profiles,
                profileIndex,
                &item,
                exception);
        if (profile != NULL) {
            const fiftyoneDegreesComponent *component =
                get_component(dataSet, profile->componentIndex);
            if (FIFTYONE_DEGREES_EXCEPTION_OKAY && component != NULL) {
                *profileId = profile->profileId;
                *componentId = component->componentId;
                result = 1;
            }
            FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->profiles, &item);
        }
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return result;
}
