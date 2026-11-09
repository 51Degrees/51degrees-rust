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
#include "constantsIpi.h"
#include "../common/constants.h"
#include "../common/collectionKeyTypes.h"

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
 * Returns the size of an IP Intelligence data file's header in bytes, being
 * how much of a data file the C library compares to find a copy of it to
 * reuse.
 */
size_t fiftyoneDegreesShimIpiGetHeaderSize(void) {
    return sizeof(fiftyoneDegreesDataSetIpiHeader);
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
 * Initialises `manager` from the `size` bytes at `memory`, the whole of a data
 * file, with a copy of `config` that neither frees the memory nor copies it to
 * a temporary file, because the memory belongs to the caller and must outlive
 * the data set. The data set reads every collection from the memory where it
 * lies, so the configuration's collection settings do not apply. The
 * configuration `config` points to is not changed.
 */
fiftyoneDegreesStatusCode fiftyoneDegreesShimIpiInitManagerFromMemory(
    fiftyoneDegreesResourceManager *manager,
    const fiftyoneDegreesConfigIpi *config,
    fiftyoneDegreesPropertiesRequired *properties,
    void *memory,
    fiftyoneDegreesFileOffset size,
    fiftyoneDegreesException *exception) {
    fiftyoneDegreesConfigIpi copy = *config;
    copy.b.freeData = false;
    copy.b.useTempFile = false;
    return fiftyoneDegreesIpiInitManagerFromMemory(
        manager,
        &copy,
        properties,
        memory,
        size,
        exception);
}

/*
 * Returns the number of handles to the data file that the data set managed
 * by `manager` opened when it loaded, which stay open until the data set is
 * freed. Every configuration that does not load the whole file into memory
 * opens one for each concurrent operation it expects, to at most 64.
 */
uint16_t fiftyoneDegreesShimIpiGetFilePoolSize(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    const uint16_t count = dataSet->b.b.filePool.pool.count;
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return count;
}

/*
 * Sets `strings` and `values` to the type names of the strings and values
 * collections of the data set managed by `manager`, and returns 1, or returns
 * 0 when there is no data set. "CollectionMemory" holds the whole collection
 * in memory, and "CollectionHeld" holds only what the required properties
 * read. The names are constants of the C library, so they outlive the data
 * set.
 */
int fiftyoneDegreesShimIpiGetCollectionTypeNames(
    fiftyoneDegreesResourceManager *manager,
    const char **strings,
    const char **values) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    *strings = dataSet->strings->typeName;
    *values = dataSet->values->typeName;
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return 1;
}

/*
 * Returns the type the data file stores the values of the required property
 * at `requiredPropertyIndex` as, read from the data set `results` was created
 * from. A latitude, for example, is stored as a Declination. Returns -1 when
 * the index is out of range, the data set does not have the property or the
 * type cannot be read.
 */
int fiftyoneDegreesShimIpiGetStoredValueType(
    fiftyoneDegreesResultsIpi *results,
    int requiredPropertyIndex) {
    if (results == NULL || results->b.dataSet == NULL) {
        return -1;
    }
    fiftyoneDegreesDataSetIpi *dataSet =
        (fiftyoneDegreesDataSetIpi *)results->b.dataSet;
    fiftyoneDegreesPropertiesAvailable *available = dataSet->b.b.available;
    if (available == NULL ||
        requiredPropertyIndex < 0 ||
        (uint32_t)requiredPropertyIndex >=
            fiftyoneDegreesPropertiesGetRequiredPropertyCount(available)) {
        return -1;
    }
    const int propertyIndex =
        fiftyoneDegreesPropertiesGetPropertyIndexFromRequiredIndex(
            available, requiredPropertyIndex);
    if (propertyIndex < 0) {
        return -1;
    }
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    const fiftyoneDegreesPropertyValueType storedType =
        fiftyoneDegreesPropertyGetStoredTypeByIndex(
            dataSet->propertyTypes,
            (uint32_t)propertyIndex,
            exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return -1;
    }
    return (int)storedType;
}

/*
 * The data file's metadata.
 *
 * The readers below give what the C++ PropertyMetaDataBuilderIpi,
 * ComponentMetaDataBuilderIpi and ValueMetaDataBuilderIpi in 51degrees-cxx
 * read, including their rules for an absent category, description, URL or
 * default value, so the crates describe each property as the other SDKs do.
 * Each string reader writes as much of the string as fits and returns its
 * full length, so the caller can read a longer one again into a buffer that
 * fits. The property values reader instead gives each string to a callback,
 * because a property can have millions of values and a value can be a long
 * area.
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
 * SHIM_VALUE_ constants, from fiftyoneDegreesShimIpiGetDataFilePropertyValues.
 */
typedef void (*fiftyoneDegreesShimValueStringCallback)(
    void *context,
    int field,
    const char *value);

/* Returned by a string reader that could not read the string. */
#define SHIM_STRING_FAILED ((size_t)-1)

/* What fiftyoneDegreesShimIpiFindDataFilePropertyValue returns. */
#define SHIM_FIND_FAILED 0
#define SHIM_FIND_FOUND 1
#define SHIM_FIND_NOT_FOUND 2

/*
 * The length of the buffer a value's text is first written into. A longer
 * text, such as an area, is written again into a buffer that fits it.
 */
#define SHIM_TEXT_BUFFER_LENGTH 256

/*
 * Writes as much of `value` as fits into `buffer` and returns the full length
 * of `value`, or writes an empty string and returns zero when `value` is null.
 */
static size_t copy_measured(char *buffer, size_t length, const char *value) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    if (value == NULL) {
        return 0;
    }
    const size_t total = strlen(value);
    if (buffer != NULL && length > 0) {
        const size_t copied = total < length - 1 ? total : length - 1;
        memcpy(buffer, value, copied);
        buffer[copied] = '\0';
    }
    return total;
}

/*
 * Writes the string at `offset` in the strings collection into `buffer` and
 * returns its full length, or SHIM_STRING_FAILED when it cannot be read.
 */
static size_t copy_string_at(
    fiftyoneDegreesDataSetIpi *dataSet,
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
    fiftyoneDegreesDataSetIpi *dataSet,
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
 * name ignoring case. Returns 1, or 0 when the name cannot be read, as the
 * C++ builder raises an exception there.
 */
static int find_available(
    fiftyoneDegreesDataSetIpi *dataSet,
    const fiftyoneDegreesProperty *property,
    uint8_t *available,
    int32_t *requiredIndex) {
    *available = 0;
    *requiredIndex = -1;
    int read = 0;
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesString *name = fiftyoneDegreesStringGet(
        dataSet->strings, property->nameOffset, &item, exception);
    if (name != NULL) {
        if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
            const char *value = FIFTYONE_DEGREES_STRING(name);
            if (dataSet->b.b.available != NULL) {
                *available =
                    fiftyoneDegreesPropertiesGetPropertyIndexFromName(
                        dataSet->b.b.available, value) >= 0 ? 1 : 0;
                *requiredIndex =
                    fiftyoneDegreesPropertiesGetRequiredPropertyIndexFromName(
                        dataSet->b.b.available, value);
            }
            read = 1;
        }
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    }
    return read;
}

/*
 * Returns the number of values the data file holds for `property`, or zero
 * when they are not all in the values collection, as when both value indexes
 * are -1, where the C++ counts none.
 */
static uint32_t value_count(
    fiftyoneDegreesDataSetIpi *dataSet,
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
    fiftyoneDegreesDataSetIpi *dataSet,
    uint32_t componentIndex) {
    if (componentIndex >= dataSet->componentsList.count) {
        return NULL;
    }
    return (fiftyoneDegreesComponent*)
        dataSet->componentsList.items[componentIndex].data.ptr;
}

/*
 * Returns the text of the value stored at `offset` in the strings collection
 * as `storedType`, with a number written to `decimalPlaces` places, as the C++
 * code writes it. The text is in `stack`, which holds `stackLength` bytes,
 * when it fits, and otherwise in a buffer this allocates, which the caller
 * frees with free_text. Returns NULL when the value cannot be read or written.
 */
static char* value_text(
    fiftyoneDegreesDataSetIpi *dataSet,
    uint32_t offset,
    fiftyoneDegreesPropertyValueType storedType,
    uint8_t decimalPlaces,
    char *stack,
    size_t stackLength) {
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    fiftyoneDegreesCollectionItem item;
    fiftyoneDegreesDataReset(&item.data);
    const fiftyoneDegreesStoredBinaryValue *value =
        fiftyoneDegreesStoredBinaryValueGet(
            dataSet->strings, offset, storedType, &item, exception);
    if (value == NULL) {
        return NULL;
    }
    char *text = NULL;
    if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
        char *buffer = stack;
        size_t length = stackLength;
        // At most three writes, as one that does not fit still counts the
        // whole length, so the second fits.
        for (int attempt = 0; attempt < 3 && text == NULL; attempt++) {
            fiftyoneDegreesStringBuilder builder = { buffer, length };
            fiftyoneDegreesStringBuilderInit(&builder);
            FIFTYONE_DEGREES_EXCEPTION_CLEAR;
            fiftyoneDegreesStringBuilderAddStringValue(
                &builder,
                value,
                storedType,
                decimalPlaces,
                exception);
            fiftyoneDegreesStringBuilderComplete(&builder);
            if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
                break;
            }
            if (builder.full == false) {
                text = buffer;
                break;
            }
            if (buffer != stack) {
                fiftyoneDegreesFree(buffer);
            }
            length = builder.added + 1;
            buffer = (char*)fiftyoneDegreesMalloc(length);
            if (buffer == NULL) {
                break;
            }
        }
        if (text == NULL && buffer != stack && buffer != NULL) {
            fiftyoneDegreesFree(buffer);
        }
    }
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    return text;
}

/* Frees text value_text allocated, being any text not in `stack`. */
static void free_text(char *text, char *stack) {
    if (text != NULL && text != stack) {
        fiftyoneDegreesFree(text);
    }
}

/*
 * Gives `callback` the string at `offset` in the strings collection as
 * `field` and returns 1, or returns 0 when the string cannot be read.
 */
static int give_string_at(
    fiftyoneDegreesDataSetIpi *dataSet,
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
 * value or one of its strings cannot be read. The name is the value written
 * as the type its property is stored as, a number to
 * FIFTYONE_DEGREES_WKT_DECIMAL_PLACES places, and the description is always
 * empty, as the C++ ValueMetaDataBuilderIpi gives them. The value is released
 * before its strings are read, so one item is held at a time.
 */
static int give_value(
    fiftyoneDegreesDataSetIpi *dataSet,
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
    const uint32_t propertyIndex = (uint32_t)value->propertyIndex;
    const int32_t urlOffset = value->urlOffsetOrWeight;
    const bool weighted = fiftyoneDegreesValueIsWeighted(value);
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
    const fiftyoneDegreesPropertyValueType storedType =
        fiftyoneDegreesPropertyGetStoredTypeByIndex(
            dataSet->propertyTypes, propertyIndex, exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return 0;
    }
    char stack[SHIM_TEXT_BUFFER_LENGTH];
    char *text = value_text(
        dataSet,
        nameOffset,
        storedType,
        FIFTYONE_DEGREES_WKT_DECIMAL_PLACES,
        stack,
        sizeof(stack));
    if (text == NULL) {
        return 0;
    }
    callback(context, SHIM_VALUE_NAME, text);
    free_text(text, stack);
    callback(context, SHIM_VALUE_DESCRIPTION, "");
    // A weighted value's URL offset holds its weight, and a value with no URL
    // points at offset 0, which holds the first string in the file.
    if (weighted == false && urlOffset > 0) {
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
 * Returns 1 when `value`, stored as `storedType`, compares equal to `name` as
 * the C library's search compares them, otherwise 0. A string is compared as
 * it is, and any other value is written into a buffer as long as `name` plus
 * three, with as many decimal places as the buffer's length, at most
 * FIFTYONE_DEGREES_MAX_DOUBLE_DECIMAL_PLACES.
 */
static int compares_equal(
    const fiftyoneDegreesStoredBinaryValue *value,
    fiftyoneDegreesPropertyValueType storedType,
    const char *name) {
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    if (storedType == FIFTYONE_DEGREES_PROPERTY_VALUE_TYPE_STRING) {
        return fiftyoneDegreesStoredBinaryValueCompareWithString(
            value, storedType, name, NULL, exception) == 0;
    }
    const size_t length = strlen(name) + 3;
    char *buffer = (char*)fiftyoneDegreesMalloc(length);
    if (buffer == NULL) {
        return 0;
    }
    fiftyoneDegreesStringBuilder builder = { buffer, length };
    fiftyoneDegreesStringBuilderInit(&builder);
    const int compared = fiftyoneDegreesStoredBinaryValueCompareWithString(
        value, storedType, name, &builder, exception);
    fiftyoneDegreesFree(buffer);
    return FIFTYONE_DEGREES_EXCEPTION_OKAY && compared == 0;
}

/*
 * Returns 1 when the value at `valueIndex` in the values collection is one of
 * `property`'s and compares equal to `name` as the C library's search
 * compares them, or 0 when it is not or cannot be read.
 */
static int is_value_named(
    fiftyoneDegreesDataSetIpi *dataSet,
    const fiftyoneDegreesProperty *property,
    fiftyoneDegreesPropertyValueType storedType,
    uint32_t valueIndex,
    const char *name) {
    if (valueIndex < property->firstValueIndex ||
        valueIndex > property->lastValueIndex) {
        return 0;
    }
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
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
    const fiftyoneDegreesStoredBinaryValue *content =
        fiftyoneDegreesStoredBinaryValueGet(
            dataSet->strings, nameOffset, storedType, &item, exception);
    if (content == NULL) {
        return 0;
    }
    int named = 0;
    if (FIFTYONE_DEGREES_EXCEPTION_OKAY) {
        named = compares_equal(content, storedType, name);
    }
    FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->strings, &item);
    return named;
}

/*
 * Finds the value `name` names among those the data file holds for
 * `property`, comparing each as the type the property is stored as, and sets
 * `index` to its index in the values collection, or -1 when the property has
 * no such value. The values are in order, so the C library's binary search
 * finds it, as the C++ ValueMetaDataCollectionBaseIpi::getByKey does. A
 * number is compared written to more places than `name` has, so the value
 * found can be written otherwise, as 0.063 is found for 0.0625. Returns 1, or
 * 0 when the property's type or a value cannot be read.
 */
static int find_value(
    fiftyoneDegreesDataSetIpi *dataSet,
    const fiftyoneDegreesProperty *property,
    const char *name,
    long *index) {
    *index = -1;
    // The search reads every place between the two value indexes, so it runs
    // only when value_count finds them all in the values collection.
    if (value_count(dataSet, property) == 0) {
        return 1;
    }
    FIFTYONE_DEGREES_EXCEPTION_CREATE;
    const fiftyoneDegreesPropertyValueType storedType =
        fiftyoneDegreesPropertyGetStoredType(
            dataSet->propertyTypes, property, exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return 0;
    }
    *index = fiftyoneDegreesValueGetIndexByNameAndType(
        dataSet->values,
        dataSet->strings,
        property,
        storedType,
        name,
        exception);
    if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
        return 0;
    }
    // When the search reads nothing for a value and sets no exception, it
    // answers as though that value matched, so the value found is compared
    // again.
    return *index < 0 ||
        is_value_named(
            dataSet, property, storedType, (uint32_t)*index, name);
}

/*
 * Sets `number` to the place of `property`'s default value among its values,
 * or -1 when it has none, and returns 1, or returns 0 when a value or a
 * string cannot be read. The C++ builder writes the value at the default
 * value index as text, a number to FIFTYONE_DEGREES_MAX_DOUBLE_DECIMAL_PLACES
 * places, and MetaDataIpi::getDefaultValueForProperty then looks that text up
 * among the property's values, so a default that is not among them is none.
 * A property with no default value index looks up an empty text, as the C++
 * does.
 */
static int default_value_number(
    fiftyoneDegreesDataSetIpi *dataSet,
    const fiftyoneDegreesProperty *property,
    int32_t *number) {
    *number = -1;
    char stack[SHIM_TEXT_BUFFER_LENGTH];
    char *text = stack;
    stack[0] = '\0';
    if (property->defaultValueIndex != UINT32_MAX) {
        FIFTYONE_DEGREES_EXCEPTION_CREATE;
        fiftyoneDegreesCollectionItem item;
        fiftyoneDegreesDataReset(&item.data);
        const fiftyoneDegreesValue *value = fiftyoneDegreesValueGet(
            dataSet->values, property->defaultValueIndex, &item, exception);
        if (value == NULL) {
            return 0;
        }
        if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
            FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
            return 0;
        }
        const uint32_t nameOffset = (uint32_t)value->nameOffset;
        const uint32_t propertyIndex = (uint32_t)value->propertyIndex;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->values, &item);
        const fiftyoneDegreesPropertyValueType storedType =
            fiftyoneDegreesPropertyGetStoredTypeByIndex(
                dataSet->propertyTypes, propertyIndex, exception);
        if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
            return 0;
        }
        text = value_text(
            dataSet,
            nameOffset,
            storedType,
            FIFTYONE_DEGREES_MAX_DOUBLE_DECIMAL_PLACES,
            stack,
            sizeof(stack));
        if (text == NULL) {
            return 0;
        }
    }
    long index = -1;
    const int found = find_value(dataSet, property, text, &index);
    free_text(text, stack);
    if (found == 0) {
        return 0;
    }
    // find_value gives only an index among the property's values.
    if (index >= 0) {
        *number = (int32_t)((uint32_t)index - property->firstValueIndex);
    }
    return 1;
}

/*
 * Returns the number of properties the data file defines, including those the
 * data set was not created with, or zero when no data set is available.
 */
uint32_t fiftyoneDegreesShimIpiGetDataFilePropertyCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = fiftyoneDegreesCollectionGetCount(dataSet->properties);
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return count;
}

/*
 * Reads the fields of the data file property at `propertyIndex` that are not
 * strings into the output parameters and returns 1, or returns 0 when the
 * index is out of range or the property, its name or its default value
 * cannot be read. The name is read to find whether the data set was created
 * with the property. `valueType` is the fiftyoneDegreesPropertyValueType the
 * property's values have. `available` is 1 when the data set was created with
 * the property, so results can hold its values, and `requiredIndex` is the
 * index the results getters take for it, or -1 when it has none.
 * `componentId` is the id of the component the property belongs to, or 0 when
 * the property names no component. `valueCount` is the number of values the
 * data file holds for the property, `defaultValueNumber` the place of its
 * default value among them, or -1 when it has none, and `mapCount` the number
 * of data tiers it is in.
 */
int fiftyoneDegreesShimIpiGetDataFileProperty(
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
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    fiftyoneDegreesCollectionItem item;
    const fiftyoneDegreesProperty *held =
        get_property(dataSet, propertyIndex, &item);
    if (held != NULL) {
        // A copy, so the default value is read with no item held.
        const fiftyoneDegreesProperty property = *held;
        FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->properties, &item);
        const fiftyoneDegreesComponent *component =
            get_component(dataSet, property.componentIndex);
        *valueType = property.valueType;
        *componentId = component == NULL ? 0 : component->componentId;
        *displayOrder = property.displayOrder;
        *isMandatory = property.isMandatory;
        *isList = property.isList;
        *isObsolete = property.isObsolete;
        *show = property.show;
        *showValues = property.showValues;
        *valueCount = value_count(dataSet, &property);
        *mapCount = property.mapCount;
        result =
            find_available(dataSet, &property, available, requiredIndex) &&
            default_value_number(dataSet, &property, defaultValueNumber);
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
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
size_t fiftyoneDegreesShimIpiGetDataFilePropertyString(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    int field,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return total;
}

/*
 * Writes the name of the data tier at `mapIndex` among those the data file
 * property at `propertyIndex` is in into `buffer`, and returns its full
 * length, or SHIM_STRING_FAILED when either index is out of range or the
 * name cannot be read.
 */
size_t fiftyoneDegreesShimIpiGetDataFilePropertyMap(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    uint32_t mapIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return total;
}

/*
 * Reads at most `count` of the values the data file holds for the property at
 * `propertyIndex`, from the one at `valueNumber` among them, in the data
 * file's order, and returns 1. For each value it calls `callback` with
 * `context` three times, being SHIM_VALUE_NAME with the value as text, then
 * SHIM_VALUE_DESCRIPTION with an empty description, then SHIM_VALUE_URL with
 * the URL, each a null terminated string valid only for that call. A value
 * with no URL gives an empty one. A `valueNumber` at or past the last value
 * reads none. The data set and the property are read once for the whole run,
 * and each value and string once. Returns 0 when the property index is out of
 * range or the property, a value or a string cannot be read, after giving
 * `callback` the strings read before the failure.
 */
int fiftyoneDegreesShimIpiGetDataFilePropertyValues(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    uint32_t valueNumber,
    uint32_t count,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return result;
}

/*
 * Finds the value `name` names among those the data file holds for the
 * property at `propertyIndex` and gives `callback` its strings as
 * fiftyoneDegreesShimIpiGetDataFilePropertyValues gives each value. Each value
 * is compared with `name` as the type its property is stored as, a number
 * written to as many places as `name` has characters plus three, at most
 * FIFTYONE_DEGREES_MAX_DOUBLE_DECIMAL_PLACES, so the text given can differ
 * from `name`, as 0.063 is given for 0.0625. A property's values are in
 * order, so the C library's binary search finds it, as the C++
 * ValueMetaDataCollectionBaseIpi::getByKey does, without reading the others.
 * Returns SHIM_FIND_FOUND, SHIM_FIND_NOT_FOUND when the property has no value
 * with that name, or SHIM_FIND_FAILED when the property index is out of range
 * or the property, a value or a string cannot be read.
 */
int fiftyoneDegreesShimIpiFindDataFilePropertyValue(
    fiftyoneDegreesResourceManager *manager,
    uint32_t propertyIndex,
    const char *name,
    fiftyoneDegreesShimValueStringCallback callback,
    void *context) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
        long index = -1;
        if (find_value(dataSet, &property, name, &index) == 1) {
            result = index < 0 ? SHIM_FIND_NOT_FOUND :
                give_value(dataSet, (uint32_t)index, callback, context) == 1 ?
                SHIM_FIND_FOUND : SHIM_FIND_FAILED;
        }
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return result;
}

/*
 * Returns the number of components the data set lists, or zero when no data
 * set is available.
 */
uint32_t fiftyoneDegreesShimIpiGetComponentCount(
    fiftyoneDegreesResourceManager *manager) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    uint32_t count = dataSet->componentsList.count;
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return count;
}

/*
 * Reads the id of the component at `componentIndex` and the offset of its
 * default profile into the output parameters and returns 1, or returns 0 when
 * the index is out of range or the default profile cannot be read. An IP
 * Intelligence data file holds no profile ids, so the C++ uses the offset as
 * the default profile's key. `hasDefaultProfile` is 1 when the component has
 * a default profile and the profile is there, and 0 otherwise, as for a
 * component whose offset is UINT32_MAX, where the C++ gives no default
 * profile.
 */
int fiftyoneDegreesShimIpiGetComponent(
    fiftyoneDegreesResourceManager *manager,
    uint32_t componentIndex,
    uint8_t *componentId,
    uint32_t *defaultProfileOffset,
    uint8_t *hasDefaultProfile) {
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
    if (dataSet == NULL) {
        return 0;
    }
    int result = 0;
    const fiftyoneDegreesComponent *component =
        get_component(dataSet, componentIndex);
    if (component != NULL) {
        const uint32_t offset = (uint32_t)component->defaultProfileOffset;
        *componentId = component->componentId;
        *defaultProfileOffset = offset;
        *hasDefaultProfile = 0;
        result = 1;
        if (offset != UINT32_MAX) {
            // Read the profile, as the C++ ProfileMetaDataCollectionIpi does,
            // which throws when the read fails.
            FIFTYONE_DEGREES_EXCEPTION_CREATE;
            fiftyoneDegreesCollectionItem item;
            fiftyoneDegreesDataReset(&item.data);
            const fiftyoneDegreesCollectionKey key = {
                { offset },
                CollectionKeyType_Profile,
            };
            const void *profile = dataSet->profiles->get(
                dataSet->profiles, &key, &item, exception);
            if (FIFTYONE_DEGREES_EXCEPTION_FAILED) {
                result = 0;
            }
            if (profile != NULL) {
                *hasDefaultProfile = result;
                FIFTYONE_DEGREES_COLLECTION_RELEASE(dataSet->profiles, &item);
            }
        }
    }
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return result;
}

/*
 * Writes the name of the component at `componentIndex` into `buffer` and
 * returns its full length, or SHIM_STRING_FAILED when the index is out of
 * range or the name cannot be read.
 */
size_t fiftyoneDegreesShimIpiGetComponentName(
    fiftyoneDegreesResourceManager *manager,
    uint32_t componentIndex,
    char *buffer,
    size_t length) {
    if (buffer != NULL && length > 0) {
        buffer[0] = '\0';
    }
    fiftyoneDegreesDataSetIpi *dataSet =
        fiftyoneDegreesDataSetIpiGet(manager);
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
    fiftyoneDegreesDataSetIpiRelease(dataSet);
    return total;
}
