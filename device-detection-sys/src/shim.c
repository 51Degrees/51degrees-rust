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
 * Writes the count of HTTP header evidence keys in the data set to *count and
 * returns a pointer that is non-null on success. The header names themselves
 * are read one at a time with fiftyoneDegreesShimHashGetEvidenceKey to avoid
 * exposing the headers collection layout.
 */
uint32_t fiftyoneDegreesShimHashGetEvidenceKeyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = 0;
    if (dataSet->b.b.uniqueHeaders != NULL) {
        count = dataSet->b.b.uniqueHeaders->count;
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Writes the name of the HTTP header evidence key at `headerIndex` into
 * `buffer` as a null terminated string and returns the number of characters
 * written, excluding the terminator. Returns zero when the index is out of
 * range, the buffer is too small or no data set is available.
 */
size_t fiftyoneDegreesShimHashGetEvidenceKey(
    fiftyoneDegreesResourceManager *manager,
    uint32_t headerIndex,
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
    fiftyoneDegreesHeaders *headers = dataSet->b.b.uniqueHeaders;
    if (headers != NULL && headerIndex < headers->count) {
        const char *value = headers->items[headerIndex].name;
        written = copy_bounded(buffer, length, value);
    }

    fiftyoneDegreesDataSetHashRelease(dataSet);
    return written;
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
 * Writes the year, month and day of the published date (`next` zero) or the
 * next update date (`next` not zero) in the data file header. Any of the
 * output pointers may be NULL.
 */
static int get_header_date(
    fiftyoneDegreesResourceManager *manager,
    int next,
    int32_t *year,
    int32_t *month,
    int32_t *day) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    const fiftyoneDegreesDate *date = next
        ? &dataSet->header.nextUpdate
        : &dataSet->header.published;
    if (year != NULL) {
        *year = (int32_t)date->year;
    }
    if (month != NULL) {
        *month = (int32_t)date->month;
    }
    if (day != NULL) {
        *day = (int32_t)date->day;
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return 1;
}

/*
 * Writes the date the data file was published, from its header, into `*year`,
 * `*month` and `*day` and returns 1, or 0 when no data set is available.
 */
int fiftyoneDegreesShimHashGetDataSetPublished(
    fiftyoneDegreesResourceManager *manager,
    int32_t *year,
    int32_t *month,
    int32_t *day) {
    return get_header_date(manager, 0, year, month, day);
}

/*
 * Writes the date the next data file is expected, from the data file header,
 * into `*year`, `*month` and `*day` and returns 1, or 0 when no data set is
 * available.
 */
int fiftyoneDegreesShimHashGetDataSetNextUpdate(
    fiftyoneDegreesResourceManager *manager,
    int32_t *year,
    int32_t *month,
    int32_t *day) {
    return get_header_date(manager, 1, year, month, day);
}

/*
 * Returns the size of a Hash data file's header in bytes, being how much of a
 * data file the C library compares to find a copy of it to reuse.
 */
size_t fiftyoneDegreesShimHashGetHeaderSize(void) {
    return sizeof(fiftyoneDegreesDataSetHashHeader);
}

/*
 * Walks the graph of each component the results have not yet walked, and
 * returns 1, or 0 when a walk fails. The walk of a component's graph sets the
 * match metrics and the matched User-Agent of its result. Unless the data set
 * was created with its properties named, a component's graph is walked only
 * when one of its properties is first read, so the readers of the results
 * below call this first. It walks nothing when no graph is left to walk.
 */
static int evaluate_all(fiftyoneDegreesResultsHash *results) {
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesResultsEvaluateAll(&results->b.b, exception);
    return FIFTYONE_DEGREES_EXCEPTION_OKAY ? 1 : 0;
}

/*
 * Reads the match metrics of the results into the output parameters and
 * returns 1, or returns 0 when `results` is NULL or a graph cannot be walked.
 * The results hold one result for each component, such as the hardware or the
 * browser, each from the header that matched it. As in the C++ ResultsHash,
 * `method` receives the highest fiftyoneDegreesHashMatchMethod of any result
 * (0 None, 1 Performance, 2 Combined, 3 Predictive), and the difference,
 * drift, iterations and matched-node count are summed across the results.
 * With no results every metric is zero, which is the method None. The metrics
 * are set by the walk of each component's graph and live on the results, not
 * in the data file's property values, so they are read here rather than
 * through the property value reader. Any output pointer may be null to skip
 * that metric.
 */
int fiftyoneDegreesShimHashGetResultMetrics(
    fiftyoneDegreesResultsHash *results,
    int32_t *method,
    int32_t *difference,
    int32_t *drift,
    int32_t *iterations,
    int32_t *matchedNodes) {
    if (results == NULL || evaluate_all(results) == 0) {
        return 0;
    }
    int32_t highestMethod = 0;
    int32_t totalDifference = 0;
    int32_t totalDrift = 0;
    int32_t totalIterations = 0;
    int32_t totalMatchedNodes = 0;
    for (uint32_t i = 0; i < results->count; i++) {
        fiftyoneDegreesResultHash *result = &results->items[i];
        if ((int32_t)result->method > highestMethod) {
            highestMethod = (int32_t)result->method;
        }
        totalDifference += result->difference;
        totalDrift += result->drift;
        totalIterations += result->iterations;
        totalMatchedNodes += result->matchedNodes;
    }
    if (method != NULL) {
        *method = highestMethod;
    }
    if (difference != NULL) {
        *difference = totalDifference;
    }
    if (drift != NULL) {
        *drift = totalDrift;
    }
    if (iterations != NULL) {
        *iterations = totalIterations;
    }
    if (matchedNodes != NULL) {
        *matchedNodes = totalMatchedNodes;
    }
    return 1;
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
 * Reads the match metrics of the single result at `resultIndex` into the
 * output parameters and returns 1, or returns 0 when the index is out of range
 * or a graph cannot be walked. Any output pointer may be null to skip that
 * metric.
 */
int fiftyoneDegreesShimHashGetResultMetricsAt(
    fiftyoneDegreesResultsHash *results,
    uint32_t resultIndex,
    int32_t *method,
    int32_t *difference,
    int32_t *drift,
    int32_t *iterations,
    int32_t *matchedNodes) {
    if (results == NULL || resultIndex >= results->count ||
        evaluate_all(results) == 0) {
        return 0;
    }
    fiftyoneDegreesResultHash *result = &results->items[resultIndex];
    if (method != NULL) {
        *method = (int32_t)result->method;
    }
    if (difference != NULL) {
        *difference = result->difference;
    }
    if (drift != NULL) {
        *drift = result->drift;
    }
    if (iterations != NULL) {
        *iterations = result->iterations;
    }
    if (matchedNodes != NULL) {
        *matchedNodes = result->matchedNodes;
    }
    return 1;
}

/*
 * Writes the matched User-Agent of the result at `resultIndex` into `buffer` as
 * a null terminated string and returns the number of characters written,
 * excluding the terminator. Returns zero when the index is out of range, a
 * graph cannot be walked or the result records no matched User-Agent, which it
 * does only when the updateMatchedUserAgent option is on, and cuts the value
 * short when the buffer is too small.
 */
size_t fiftyoneDegreesShimHashGetMatchedUserAgent(
    fiftyoneDegreesResultsHash *results,
    uint32_t resultIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    if (results == NULL || resultIndex >= results->count ||
        evaluate_all(results) == 0) {
        return 0;
    }
    return copy_bounded(
        buffer,
        length,
        results->items[resultIndex].b.matchedUserAgent);
}

/*
 * Returns the number of handles in the file pool of the data set managed by
 * `manager` when any of its collections reads from the data file after it is
 * loaded, or zero when the whole data set is held in memory. A read that
 * finds every handle in use fails rather than waiting for one, so this is
 * how many calls can read the data file at once.
 */
uint16_t fiftyoneDegreesShimHashGetFileHandleCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    const fiftyoneDegreesConfigHash *config = &dataSet->config;
    const bool readsFile = dataSet->b.b.isInMemory == false && (
        config->strings.loaded == false ||
        config->components.loaded == false ||
        config->maps.loaded == false ||
        config->properties.loaded == false ||
        config->values.loaded == false ||
        config->profiles.loaded == false ||
        config->rootNodes.loaded == false ||
        config->nodes.loaded == false ||
        config->profileOffsets.loaded == false);
    const uint16_t count = readsFile ? dataSet->b.b.filePool.pool.count : 0;
    fiftyoneDegreesDataSetHashRelease(dataSet);
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
 * SHIM_VALUE_ constants, from fiftyoneDegreesShimHashGetDataFilePropertyValues.
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
 * it, otherwise 0, and `requiredIndex` to its required property index, which
 * the results getters take, or -1 when it has none. The C library matches the
 * name ignoring case, so a property named in another case is found. Returns
 * 1, or 0 when the name cannot be read, as the C++ builder raises an
 * exception there.
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
            *available = fiftyoneDegreesPropertiesGetPropertyIndexFromName(
                dataSet->b.b.available, value) >= 0 ? 1 : 0;
            *requiredIndex =
                fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
                    dataSet->b.b.available, value);
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
 * index is out of range or the property or its name cannot be read. The
 * name is read to find whether the data set was created with the property.
 * `valueType` is the fiftyoneDegreesPropertyValueType the property's values
 * have. `available` is 1 when the data set was created with the property, so
 * results can hold its values, and `requiredIndex` is the index the results
 * getters take for it, or -1 when it has none. `componentId` is the id of the
 * component the property belongs to, or 0 when the property names no
 * component.
 * `valueCount` is the number of values the data file holds for the property,
 * `defaultValueNumber` the place of its default value among them, or -1 when
 * it has none, and `mapCount` the number of data tiers it is in.
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
        const uint32_t first = property->firstValueIndex;
        const uint32_t count = value_count(dataSet, property);
        *valueCount = count;
        // The C++ MetaDataHash::getDefaultValueForProperty looks the default
        // value up by name among the property's values, so a default outside
        // them is none.
        *defaultValueNumber = property->defaultValueIndex >= first &&
            property->defaultValueIndex - first < count ?
            (int32_t)(property->defaultValueIndex - first) : -1;
        *mapCount = property->mapCount;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
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
 * fiftyoneDegreesShimHashGetDataFilePropertyValues gives each value. The name
 * must match exactly. A property's values are in order of their names, so the
 * C library's binary search finds it, as the C++
 * ValueMetaDataCollectionForPropertyHash::getByKey does, without reading the
 * others. A value the data file holds out of that order can be missed, as the
 * C++ misses it. Returns SHIM_FIND_FOUND, SHIM_FIND_NOT_FOUND when the property
 * has no value with that name, or SHIM_FIND_FAILED when the property index is
 * out of range or the property, a value or a string cannot be read.
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
        // A copy, so only one item is held at a time during the search.
        const fiftyoneDegreesProperty property = *held;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
        result = SHIM_FIND_NOT_FOUND;
        // The search reads every place between the two value indexes, so it
        // runs only when value_count finds them all in the values collection.
        if (value_count(dataSet, &property) > 0) {
            FIFTYONE_DEGREES_EXCEPTION_CREATE;
            const long index = fiftyoneDegreesValueGetIndexByNameAndType(
                dataSet->values,
                dataSet->strings,
                &property,
                FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_STRING,
                name,
                exception);
            if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
                result = SHIM_FIND_FAILED;
            }
            else if (index >= 0) {
                result = give_value(
                    dataSet, (uint32_t)index, callback, context) == 1 ?
                    SHIM_FIND_FOUND : SHIM_FIND_FAILED;
            }
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
