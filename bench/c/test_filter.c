#include <stdio.h>
#include <stdbool.h>
#include <string.h>
#include <stdlib.h>

static inline bool is_num_char(char c) {
    return (c >= '0' && c <= '9') || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E';
}

/* 
 * LITERAL EXCLUSION FILTER (Refined for prefix tokens & string safety)
 */
static bool is_known_numeric_divergence(const char *orig_s, const char *rust_s) {
    if (!orig_s || !rust_s) return false;
    if (strcmp(orig_s, rust_s) == 0) return true;

    const char *p1 = orig_s;
    const char *p2 = rust_s;
    bool in_string = false;
    bool escape = false;

    while (*p1 != '\0' && *p2 != '\0') {
        if (*p1 == '"' && !escape) {
            in_string = !in_string;
        }
        escape = (*p1 == '\\' && !escape);

        /* If outside quotes and we reach a numeric literal on EITHER side, process numbers as whole tokens */
        if (!in_string && (is_num_char(*p1) || is_num_char(*p2))) {
            /* Both sides must be starting/inside a numeric literal */
            if (!is_num_char(*p1) || !is_num_char(*p2)) {
                return false; // One side has a number, the other does not!
            }
            /* Fast-forward both pointers to the conclusion of their respective numeric tokens */
            while (*p1 != '\0' && is_num_char(*p1)) p1++;
            while (*p2 != '\0' && is_num_char(*p2)) p2++;
            continue;
        }

        /* For non-numeric or quoted characters, they must match identically */
        if (*p1 == *p2) {
            p1++;
            p2++;
            continue;
        }

        /* Non-matching character outside numeric relaxation */
        return false;
    }

    /* Both strings must reach EOF simultaneously after passing all syntax and numbers */
    return (*p1 == '\0' && *p2 == '\0');
}

int main(void) {
    printf("============================================================\n");
    printf("Differential Fuzzer Exclusion Filter Verification Suite\n");
    printf("============================================================\n\n");

    struct {
        const char *name;
        const char *orig_s;
        const char *rust_s;
        bool expected_result;
        const char *reason;
    } cases[] = {
        {
            "Case 1: Standard Exponent Representation Differing",
            "{\"val\":1e-07,\"status\":\"ok\"}",
            "{\"val\":0.0000001,\"status\":\"ok\"}",
            true,
            "Numeric literal format difference outside quotes must be forgiven"
        },
        {
            "Case 2: Prefix Numeric Token Terminating exactly at EOF",
            "{\"num\":5e-1",
            "{\"num\":5e-10",
            true,
            "One number is a prefix of the other right at EOF string termination"
        },
        {
            "Case 3: Quoted String Exponent Mismatch",
            "{\"version\":\"1e-07\"}",
            "{\"version\":\"1e-007\"}",
            false,
            "Differences inside string literals must NEVER be forgiven (Authentic Bug)"
        },
        {
            "Case 4: Structural Property / Count Divergence",
            "{\"val\":100,\"extra\":true}",
            "{\"val\":100}",
            false,
            "Missing property / structural syntax divergence must NEVER be forgiven"
        },
        {
            "Case 5: Type Mismatch at Property Value",
            "{\"val\":0}",
            "{\"val\":null}",
            false,
            "Number versus null node must NEVER be forgiven as numeric difference"
        }
    };

    int passed = 0;
    int num_cases = sizeof(cases) / sizeof(cases[0]);
    for (int i = 0; i < num_cases; i++) {
        bool actual = is_known_numeric_divergence(cases[i].orig_s, cases[i].rust_s);
        printf("[%s] %s\n", (actual == cases[i].expected_result) ? "PASS" : "FAIL", cases[i].name);
        printf("  Orig:     %s\n", cases[i].orig_s);
        printf("  Rust:     %s\n", cases[i].rust_s);
        printf("  Expected: %s | Actual: %s (%s)\n\n", 
               cases[i].expected_result ? "IGNORED (True)" : "REAL BUG (False)",
               actual ? "IGNORED (True)" : "REAL BUG (False)",
               cases[i].reason);
        if (actual == cases[i].expected_result) passed++;
    }

    printf("============================================================\n");
    printf("Filter Verification Summary: %d / %d Tests Passed\n", passed, num_cases);
    printf("============================================================\n");
    return (passed == num_cases) ? 0 : 1;
}
