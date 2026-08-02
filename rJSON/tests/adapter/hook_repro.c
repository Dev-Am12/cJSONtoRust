#include <stdlib.h>

#include "cJSON.h"

static void *CJSON_CDECL failing_malloc(size_t size)
{
    (void)size;
    return NULL;
}

static void CJSON_CDECL normal_free(void *pointer)
{
    free(pointer);
}

static size_t counted_alloc_calls = 0;
static size_t counted_free_calls = 0;
static size_t counted_fail_after = 0;

static void *CJSON_CDECL counted_malloc(size_t size)
{
    counted_alloc_calls++;
    if (counted_alloc_calls > counted_fail_after)
    {
        return NULL;
    }
    return malloc(size);
}

static void CJSON_CDECL counted_free(void *pointer)
{
    if (pointer != NULL)
    {
        counted_free_calls++;
    }
    free(pointer);
}

int main(void)
{
    int numbers[] = {1, 2, 3};
    cJSON_Hooks hooks = {failing_malloc, normal_free};
    cJSON_Hooks counted_hooks = {counted_malloc, counted_free};

    cJSON_InitHooks(&hooks);
    cJSON *array = cJSON_CreateIntArray(numbers, 3);
    cJSON_InitHooks(NULL);

    if (array != NULL)
    {
        cJSON_Delete(array);
        return 1;
    }

    counted_alloc_calls = 0;
    counted_free_calls = 0;
    counted_fail_after = 4;

    cJSON_InitHooks(&counted_hooks);
    cJSON *tree = cJSON_Parse("{\"a\":[1,2],\"b\":\"x\"}");
    cJSON_InitHooks(NULL);

    if (tree != NULL)
    {
        cJSON_Delete(tree);
        return 2;
    }

    if (counted_alloc_calls <= counted_fail_after)
    {
        return 3;
    }

    if (counted_free_calls != counted_fail_after)
    {
        return 4;
    }

    return 0;
}
