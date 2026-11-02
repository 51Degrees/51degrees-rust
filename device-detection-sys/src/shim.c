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
 * Returns the number of required (available) properties in the data set managed
 * by `manager`, or zero if the data set or its available properties are not
 * present. The data set reference is taken and released around the read.
 */
uint32_t fiftyoneDegreesShimHashGetRequiredPropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetHash *dataSet =
        fiftyoneDegreesDataSetHashGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = 0;
    if (dataSet->b.b.available != NULL) {
        count = dataSet->b.b.available->count;
    }
    fiftyoneDegreesDataSetHashRelease(dataSet);
    return count;
}

/*
 * Writes the name of the required property at `requiredPropertyIndex` into
 * `buffer` as a null terminated string and returns the number of characters
 * written, excluding the terminator. Returns zero when the index is out of
 * range, the buffer is too small or no data set is available. The buffer is
 * always null terminated when `length` is at least one.
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
        (uint32_t)requiredPropertyIndex < available->count) {
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
