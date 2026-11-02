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
 * Property and header enumeration shim for fiftyone-ip-intelligence-sys.
 *
 * The set of properties an IP Intelligence data set was initialized with, and
 * the headers it reads the client IP from, live in the `available` and
 * `uniqueHeaders` members of the base data set, several layers of nested
 * structures deep (DataSetIpi -> DataSetIpiBase b -> DataSetBase b). Reaching
 * them from Rust would require mirroring every intermediate C struct exactly,
 * which is fragile across platforms and compiler versions. The helpers below
 * read those private layouts from C, where the headers describe them, and
 * present a flat C ABI to the Rust crate. They use only the public IP
 * Intelligence and common C library headers, so they stay correct as the
 * upstream layouts change.
 *
 * This mirrors the equivalent shim in fiftyone-device-detection-sys. The Hash
 * data set nests as DataSetHash -> DataSetHashBase b -> DataSetBase b, so the
 * `dataSet->b.b` access path is the same in both engines.
 */

#include "ipi.h"

/*
 * Returns the number of required property indexes of the data set managed by
 * `manager`, which is the number of names it was created with, or when it was
 * created naming no property, the number of its available properties. Returns
 * zero if the data set or its available properties are not present. The data
 * set reference is taken and released around the read.
 */
uint32_t fiftyoneDegreesShimIpiGetRequiredPropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = fiftyoneDegreesPropertiesGetRequiredPropertyCount(
        dataSet->b.b.available);
    fiftyoneDegreesDataSetIpiRelease(dataSet);
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
size_t fiftyoneDegreesShimIpiGetRequiredPropertyName(
    fiftyoneDegreesResourceManager *manager,
    int requiredPropertyIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }

    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
        if (value != NULL && buffer != NULL && length > 0) {
            size_t i = 0;
            while (value[i] != '\0' && i < length - 1) {
                buffer[i] = value[i];
                i++;
            }
            buffer[i] = '\0';
            written = i;
        }
    }

    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return written;
}

/*
 * Returns the zero based required property index for the property named
 * `propertyName`, or -1 when the property is not one of the required
 * properties. This index is the value expected by the results getters such as
 * fiftyoneDegreesResultsIpiGetHasValues and the weighted collection getter
 * fiftyoneDegreesResultsIpiGetValuesCollection.
 */
int fiftyoneDegreesShimIpiGetRequiredPropertyIndexFromName(
    fiftyoneDegreesResourceManager *manager,
    const char *propertyName) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return -1;
    }
    int index = -1;
    if (dataSet->b.b.available != NULL) {
        index = fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
            dataSet->b.b.available, propertyName);
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return index;
}

/*
 * Writes the data set's name into `buffer` as a null terminated string and
 * returns the number of characters written, excluding the terminator. The data
 * set name is the file's tier, for example "Lite", "Enterprise" or "TAC".
 * Returns zero when no data set is available, the name cannot be read, or the
 * buffer is too small. The buffer is always null terminated when `length` is at
 * least one.
 *
 * The name lives in the strings collection at `header.nameOffset`. This mirrors
 * the Hash engine's data set name accessor in fiftyone-device-detection-sys; the
 * IP Intelligence header carries the same `nameOffset` field.
 */
size_t fiftyoneDegreesShimIpiGetDataSetName(
    fiftyoneDegreesResourceManager *manager,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }

    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
            const char *value = FIFTYONE_DEGREES_STRING(name);
            if (value != NULL && buffer != NULL && length > 0) {
                size_t i = 0;
                while (value[i] != '\0' && i < length - 1) {
                    buffer[i] = value[i];
                    i++;
                }
                buffer[i] = '\0';
                written = i;
            }
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }

    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return written;
}

/*
 * Writes the data set's published date into `*year`, `*month` and `*day` and
 * returns 1 on success, or 0 when no data set is available. Any of the output
 * pointers may be NULL. The date is read from the data set header, which carries
 * the embedded publish date of the data file.
 */
int fiftyoneDegreesShimIpiGetDataSetPublished(
    fiftyoneDegreesResourceManager *manager,
    int32_t *year,
    int32_t *month,
    int32_t *day) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    if (year != NULL) {
        *year = (int32_t)dataSet->header.published.year;
    }
    if (month != NULL) {
        *month = (int32_t)dataSet->header.published.month;
    }
    if (day != NULL) {
        *day = (int32_t)dataSet->header.published.day;
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return 1;
}

/*
 * Returns the number of headers the data set reads the client IP from, or zero
 * when no data set is available. Their names are read one at a time with
 * fiftyoneDegreesShimIpiGetUniqueHeaderName, in the data set's priority order.
 */
uint32_t fiftyoneDegreesShimIpiGetUniqueHeaderCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = 0;
    if (dataSet->b.b.uniqueHeaders != NULL) {
        count = dataSet->b.b.uniqueHeaders->count;
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return count;
}

/*
 * Writes the name of the header at `headerIndex` into `buffer` as a null
 * terminated string and returns the number of characters written, excluding
 * the terminator. Returns zero when the index is out of range or no data set is
 * available, and cuts the name short when the buffer is too small. The buffer
 * is always null terminated when `length` is at least one.
 */
size_t fiftyoneDegreesShimIpiGetUniqueHeaderName(
    fiftyoneDegreesResourceManager *manager,
    uint32_t headerIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }

    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }

    size_t written = 0;
    fiftyoneDegreesHeaders *headers = dataSet->b.b.uniqueHeaders;
    if (headers != NULL && headerIndex < headers->count) {
        const char *value = headers->items[headerIndex].name;
        if (value != NULL && buffer != NULL && length > 0) {
            size_t i = 0;
            while (value[i] != '\0' && i < length - 1) {
                buffer[i] = value[i];
                i++;
            }
            buffer[i] = '\0';
            written = i;
        }
    }

    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return written;
}

/*
 * Initialises `manager` from the data file `fileName` with a copy of
 * `config`, setting the expected concurrency of every collection in the copy
 * to `concurrency` when that is zero or more. The expected concurrency sizes
 * the pool of file handles the collections that are not held in memory read
 * through. The configuration `config` points to is not changed.
 */
fiftyoneDegreesStatusCode fiftyoneDegreesShimIpiInitManagerFromFile(
    fiftyoneDegreesResourceManager *manager,
    const fiftyoneDegreesConfigIpi *config,
    int32_t concurrency,
    fiftyoneDegreesPropertiesRequired *properties,
    const char *fileName,
    fiftyoneDegreesException *exception) {
    fiftyoneDegreesConfigIpi copy = *config;
    if (concurrency >= 0) {
        const uint16_t expected = (uint16_t)concurrency;
        copy.strings.concurrency = expected;
        copy.components.concurrency = expected;
        copy.maps.concurrency = expected;
        copy.properties.concurrency = expected;
        copy.values.concurrency = expected;
        copy.profiles.concurrency = expected;
        copy.graphs.concurrency = expected;
        copy.profileGroups.concurrency = expected;
        copy.profileOffsets.concurrency = expected;
        copy.propertyTypes.concurrency = expected;
        copy.graph.concurrency = expected;
    }
    return fiftyoneDegreesIpiInitManagerFromFile(
        manager,
        &copy,
        properties,
        fileName,
        exception);
}

/*
 * Returns the number of handles in the file pool of the data set managed by
 * `manager` when any of its collections reads from the data file after it is
 * loaded, or zero when the whole data set is held in memory. A read that
 * finds every handle in use fails rather than waiting for one, so this is
 * how many calls can read the data file at once.
 */
uint16_t fiftyoneDegreesShimIpiGetFileHandleCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    const fiftyoneDegreesConfigIpi *config = &dataSet->config;
    const bool readsFile = dataSet->b.b.isInMemory == false && (
        config->strings.loaded == false ||
        config->components.loaded == false ||
        config->maps.loaded == false ||
        config->properties.loaded == false ||
        config->values.loaded == false ||
        config->profiles.loaded == false ||
        config->graphs.loaded == false ||
        config->profileGroups.loaded == false ||
        config->profileOffsets.loaded == false ||
        config->propertyTypes.loaded == false ||
        config->graph.loaded == false);
    const uint16_t count = readsFile ? dataSet->b.b.filePool.pool.count : 0;
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return count;
}
