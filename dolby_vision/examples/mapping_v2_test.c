/* Compile against the generated rpu_parser.h and link the library built with capi.
 * Usage: mapping_v2_test AUTHORED_RPU
 * Exercises mixed methods, independent coefficient association and owner lifetime. */
#include "rpu_parser.h"
#include <stdio.h>

#define CHECK(condition) do { if (!(condition)) { \
    fprintf(stderr, "mapping v2 check failed at line %d\n", __LINE__); \
    goto cleanup; \
} } while (0)

int main(int argc, char **argv) {
    int result = 1;
    FILE *file = NULL;
    DoviRpuOpaque *rpu = NULL;
    const DoviRpuDataMappingV2 *mapping = NULL;
    uint8_t bytes[8192];
    size_t length = 0;
    CHECK(argc == 2);
    file = fopen(argv[1], "rb");
    CHECK(file != NULL);
    length = fread(bytes, 1, sizeof(bytes), file);
    CHECK(!ferror(file) && feof(file) && length > 0);
    fclose(file);
    file = NULL;
    rpu = dovi_parse_rpu(bytes, length);
    CHECK(rpu != NULL);
    mapping = dovi_rpu_get_data_mapping_v2(rpu);
    CHECK(mapping != NULL);
    dovi_rpu_free(rpu);
    rpu = NULL;

    for (size_t cmp = 0; cmp < 3; cmp++) {
        const DoviReshapingCurveV2 *curve = &mapping->curves[cmp];
        CHECK(curve->num_pivots_minus2 == 2 && curve->piece_count == 3);
        CHECK(curve->pivots[0] == 0 && curve->pivots[1] == 200);
        CHECK(curve->pivots[2] == 300 && curve->pivots[3] == 523);
        for (size_t i = 0; i < curve->piece_count; i++) {
            const DoviMappingPieceV2 *piece = &curve->pieces[i];
            uint64_t marker = 100 * cmp + 10 * i + 1;
            CHECK(piece->method == (cmp + i) % 2);
            if (piece->method == 0) {
                CHECK(piece->polynomial_count == 2 && piece->mmr_order_count == 0);
                for (size_t j = 0; j < 2; j++) {
                    CHECK(piece->polynomial[j].has_integer == 1);
                    CHECK(piece->polynomial[j].integer == -(int64_t)(marker + j));
                    CHECK(piece->polynomial[j].value == marker + j);
                }
            } else {
                CHECK(piece->polynomial_count == 0 && piece->mmr_order_count == i % 3 + 1);
                CHECK(piece->mmr_constant.has_integer == 1);
                CHECK(piece->mmr_constant.integer == -(int64_t)marker);
                CHECK(piece->mmr_constant.value == marker);
                for (size_t order = 0; order < piece->mmr_order_count; order++) {
                    for (size_t j = 0; j < 7; j++) {
                        uint64_t expected = marker + 1 + order * 7 + j;
                        CHECK(piece->mmr_orders[order][j].has_integer == 1);
                        CHECK(piece->mmr_orders[order][j].integer == -(int64_t)expected);
                        CHECK(piece->mmr_orders[order][j].value == expected);
                    }
                }
            }
        }
    }
    CHECK(dovi_rpu_get_data_mapping_v2(NULL) == NULL);
    result = 0;
cleanup:
    if (file) fclose(file);
    if (rpu) dovi_rpu_free(rpu);
    dovi_rpu_free_data_mapping_v2(mapping);
    dovi_rpu_free_data_mapping_v2(NULL);
    return result;
}
