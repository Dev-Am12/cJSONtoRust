#define _POSIX_C_SOURCE 199309L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>
#include "cJSON.h"

static double get_time_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1000000.0 + (double)ts.tv_nsec / 1000.0;
}

static int compare_doubles(const void *a, const void *b) {
    double da = *(const double *)a;
    double db = *(const double *)b;
    if (da < db) return -1;
    if (da > db) return 1;
    return 0;
}

static char* load_file(const char* path, size_t* out_size) {
    FILE* f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "Failed to open file: %s\n", path);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (len < 0) {
        fclose(f);
        exit(1);
    }
    char* buf = malloc((size_t)len + 1);
    if (!buf) {
        fclose(f);
        exit(1);
    }
    size_t read_bytes = fread(buf, 1, (size_t)len, f);
    buf[read_bytes] = '\0';
    fclose(f);
    if (out_size) *out_size = read_bytes;
    return buf;
}

int main(int argc, char** argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <file_path> <iterations> [label]\n", argv[0]);
        return 1;
    }

    const char* path = argv[1];
    int iterations = atoi(argv[2]);
    if (iterations <= 0) iterations = 100;
    const char* label = (argc > 3) ? argv[3] : "cjson_c";

    size_t file_size = 0;
    char* payload = load_file(path, &file_size);

    // Warmup phase (100 iterations)
    for (int i = 0; i < 100; i++) {
        cJSON* item = cJSON_Parse(payload);
        if (item) {
            cJSON_Delete(item);
        }
    }

    double* times_us = malloc((size_t)iterations * sizeof(double));
    if (!times_us) {
        free(payload);
        return 1;
    }

    for (int i = 0; i < iterations; i++) {
        double start = get_time_us();
        cJSON* item = cJSON_Parse(payload);
        if (!item) {
            fprintf(stderr, "Parse failed during benchmark on iteration %d!\n", i);
            free(times_us);
            free(payload);
            return 1;
        }
        cJSON_Delete(item);
        double elapsed = get_time_us() - start;
        times_us[i] = elapsed;
    }

    qsort(times_us, (size_t)iterations, sizeof(double), compare_doubles);

    double sum = 0.0;
    for (int i = 0; i < iterations; i++) {
        sum += times_us[i];
    }
    double mean = sum / (double)iterations;
    double min = times_us[0];
    double max = times_us[iterations - 1];
    double median = (iterations % 2 == 0)
        ? (times_us[iterations / 2 - 1] + times_us[iterations / 2]) / 2.0
        : times_us[iterations / 2];

    double variance_sum = 0.0;
    for (int i = 0; i < iterations; i++) {
        double diff = times_us[i] - mean;
        variance_sum += diff * diff;
    }
    double std_dev = sqrt(variance_sum / (double)iterations);

    const char* filename = strrchr(path, '/');
    if (!filename) filename = strrchr(path, '\\');
    filename = filename ? (filename + 1) : path;

    printf("%-15s | File: %-15s | Mean: %8.2f us | Median: %8.2f us | Min: %8.2f us | Max: %8.2f us | StdDev: %6.2f us | Iters: %d\n",
           label, filename, mean, median, min, max, std_dev, iterations);

    // Output JSON record line for results.json aggregation
    printf("{\"api\": \"%s\", \"file\": \"%s\", \"size_bytes\": %lu, \"iterations\": %d, \"mean_us\": %.2f, \"median_us\": %.2f, \"min_us\": %.2f, \"max_us\": %.2f, \"std_dev_us\": %.2f}\n",
           label, filename, (unsigned long)file_size, iterations, mean, median, min, max, std_dev);

    free(times_us);
    free(payload);
    return 0;
}
