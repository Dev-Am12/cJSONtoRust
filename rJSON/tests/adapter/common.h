/*
 * adapter/common.h — drop-in replacement for tests/original/common.h
 *
 * Purpose: allows the 6 adapter-eligible original C test files to compile
 * against the rJSON Rust cdylib (librjson) without any modification.
 *
 * Key differences from original common.h:
 *   - Does NOT #include "../cJSON.c" (the C implementation)
 *   - Instead includes our facade cJSON.h (declarations only)
 *   - Does NOT define reset() — it uses cJSON-internal global_hooks which
 *     has no equivalent in our facade; none of the 6 files call reset()
 *   - Defines read_file() and compare_double() which several test files need
 *
 * Compile with -I rJSON/tests/adapter/ BEFORE -I rJSON/tests/original/
 * so this file shadows the original common.h.
 */

#ifndef CJSON_ADAPTER_COMMON_H
#define CJSON_ADAPTER_COMMON_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

/* Pull in our facade declarations (struct cJSON, all function prototypes). */
#include "cJSON.h"

/* ---------------------------------------------------------------------------
 * read_file — used by parse_examples.c and readme_examples.c
 * Identical to the original common.h implementation.
 * --------------------------------------------------------------------------- */
static char* read_file(const char *filename)
{
    FILE *file = NULL;
    long length = 0;
    char *content = NULL;
    size_t read_chars = 0;

    file = fopen(filename, "rb");
    if (file == NULL)
        goto cleanup;

    if (fseek(file, 0, SEEK_END) != 0)
        goto cleanup;

    length = ftell(file);
    if (length < 0)
        goto cleanup;

    if (fseek(file, 0, SEEK_SET) != 0)
        goto cleanup;

    content = (char*)malloc((size_t)length + sizeof(""));
    if (content == NULL)
        goto cleanup;

    read_chars = fread(content, sizeof(char), (size_t)length, file);
    if ((long)read_chars != length)
    {
        free(content);
        content = NULL;
        goto cleanup;
    }
    content[read_chars] = '\0';

cleanup:
    if (file != NULL)
        fclose(file);
    return content;
}

/* ---------------------------------------------------------------------------
 * compare_double — used by readme_examples.c
 * --------------------------------------------------------------------------- */
static int compare_double(double a, double b)
{
    double maxVal = fabs(a) > fabs(b) ? fabs(a) : fabs(b);
    return (fabs(a - b) <= maxVal * DBL_EPSILON);
}

#endif /* CJSON_ADAPTER_COMMON_H */
