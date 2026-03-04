/*
 * SIMD-Accelerated Sieve of Eratosthenes
 * AVX2 Implementation for 100M+ Primes
 * 
 * Optimizations:
 * - Bit-packed odd-only representation (6.25MB for 100M)
 * - Cache-line aligned blocks (64 bytes)
 * - AVX2 parallel marking with _mm256_andnot
 * - Prefetching for streaming access
 * 
 * Compile: gcc -O3 -mavx2 -mtune=native -o sieve sieve.c
 */

#include <immintrin.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <time.h>

#define LIMIT 100000000          // 100M
#define ALIGN64(x) __attribute__((aligned(64)))
#define CACHE_LINE_SIZE 64
#define BLOCK_SIZE 32            // Process 32 bits per AVX2 iteration

typedef uint8_t sieve_t;

// Memory layout: 1 bit per odd number
// Bit n → odd number (2n + 3)
#define NUM_CANDIDATES ((LIMIT - 1) / 2)
#define SIEVE_SIZE ((NUM_CANDIDATES + 7) / 8)

ALIGN64(sieve_t sieve[SIEVE_SIZE]);

// Bit manipulation helpers
static inline int bit_to_odd(int bit_idx) {
    return (bit_idx * 2) + 3;  // Map bit index to odd number
}

static inline void set_bit(sieve_t *sieve, int bit_idx) {
    sieve[bit_idx / 8] |= (1 << (bit_idx % 8));
}

static inline void clear_bit(sieve_t *sieve, int bit_idx) {
    sieve[bit_idx / 8] &= ~(1 << (bit_idx % 8));
}

static inline int is_bit_set(const sieve_t *sieve, int bit_idx) {
    return (sieve[bit_idx / 8] >> (bit_idx % 8)) & 1;
}

// Generate mask for marking multiples of prime p
// stride = (p - 3) / 2 for odd-only representation
static inline __mmask32 generate_stride_mask(int stride) {
    __mmask32 mask = 0;
    for (int i = 0; i < 32; i++) {
        if (i % stride == 0) {
            mask |= (1 << i);
        }
    }
    return mask;
}

// AVX2-optimized sieve block processing
static inline void sieve_block_avx2(
    sieve_t *sieve,
    int start_bit,
    int count,
    int stride
) {
    int end_bit = start_bit + count;
    int bit_idx = start_bit;
    
    // Process in 32-bit blocks
    while (bit_idx + BLOCK_SIZE <= end_bit) {
        // Calculate byte offset (each bit is 1/8 byte)
        int byte_offset = bit_idx / 8;
        
        // Load 32 bits (4 bytes) into AVX2 register
        __m256i block = _mm256_load_si256(
            (__m256i*)(sieve + byte_offset)
        );
        
        // Generate mask for this stride pattern
        __mmask32 mask = generate_stride_mask(stride);
        
        // Apply mask: clear bits where mask is 1
        // _mm256_andnot_si256(a, b) = a & (~b)
        block = _mm256_andnot_si256(block, _mm256_set_m32(mask));
        
        // Store back (cache-line aligned)
        _mm256_store_si256(
            (__m256i*)(sieve + byte_offset),
            block
        );
        
        bit_idx += BLOCK_SIZE;
    }
    
    // Handle remainder with scalar code
    while (bit_idx < end_bit) {
        int bits_remaining = end_bit - bit_idx;
        int byte_offset = bit_idx / 8;
        
        __m256i block = _mm256_load_si256(
            (__m256i*)(sieve + byte_offset)
        );
        
        __mmask32 mask = generate_stride_mask(stride);
        __mmask32 lower_mask = mask & ((1 << bits_remaining) - 1);
        
        block = _mm256_andnot_si256(block, _mm256_set_m32(mask));
        
        _mm256_store_si256(
            (__m256i*)(sieve + byte_offset),
            block
        );
        
        bit_idx++;
    }
}

// Generate base primes up to sqrt(limit)
static void generate_base_primes(int *base_primes, int *count, int limit) {
    int sqrt_limit = (int)sqrt(limit);
    int num_base_odds = (sqrt_limit - 1) / 2;
    
    // Small sieve for base primes
    sieve_t base_sieve[(num_base_odds + 7) / 8];
    memset(base_sieve, 0xff, sizeof(base_sieve));  // All set (prime)
    base_sieve[0] = 0;  // 3 is composite (divisible by 3)
    
    // Sieve base primes
    for (int i = 1; i * i <= num_base_odds; i++) {
        if (is_bit_set(base_sieve, i)) {
            int odd_prime = (i * 2) + 3;
            int stride = i;  // stride in bit indices
            
            // Mark multiples
            for (int j = i * i; j < num_base_odds; j += stride) {
                clear_bit(base_sieve, j);
            }
        }
    }
    
    // Collect base primes
    *count = 0;
    for (int i = 0; i < num_base_odds; i++) {
        if (is_bit_set(base_sieve, i)) {
            base_primes[(*count)++] = (i * 2) + 3;
        }
    }
}

// Main sieve function with AVX2
void sieve_primes_avx2(int *base_primes, int base_count) {
    // Initialize sieve: all bits set to 1 (prime)
    memset(sieve, 0xff, SIEVE_SIZE);
    
    // Mark 3 as composite (already done in base sieve, but ensure)
    clear_bit(sieve, 0);
    
    // Sieve with base primes
    for (int i = 0; i < base_count; i++) {
        int prime = base_primes[i];
        
        if (prime * prime > LIMIT) break;
        
        // Calculate stride in bit indices
        int stride = (prime - 3) / 2;
        
        // Process entire sieve with AVX2
        sieve_block_avx2(sieve, 0, NUM_CANDIDATES, stride);
    }
}

// Count primes in sieve
int count_primes(const sieve_t *sieve) {
    int count = 1;  // Count 2 (the only even prime)
    
    // Count set bits in sieve
    for (int i = 0; i < NUM_CANDIDATES; i++) {
        if (is_bit_set(sieve, i)) {
            count++;
        }
    }
    
    return count;
}

// Extract primes to array
void extract_primes(const sieve_t *sieve, int *primes, int count) {
    int idx = 0;
    
    // Add 2
    primes[idx++] = 2;
    
    // Extract odd primes
    for (int i = 0; i < NUM_CANDIDATES; i++) {
        if (is_bit_set(sieve, i)) {
            primes[idx++] = bit_to_odd(i);
        }
    }
}

// Print statistics
void print_stats(int limit, int count) {
    printf("Sieve Statistics:\n");
    printf("  Limit: %d\n", limit);
    printf("  Primes found: %d\n", count);
    printf("  Memory used: %d bytes (%.2f MB)\n", 
           SIEVE_SIZE, SIEVE_SIZE / (1024.0 * 1024.0));
    printf("  Density: %.4f%%\n", (count * 100.0) / limit);
}

// Print first/last primes
void print_sample_primes(const sieve_t *sieve, int count) {
    int primes[1000];
    extract_primes(sieve, primes, count);
    
    printf("\nFirst 10 primes: ");
    for (int i = 0; i < 10 && i < count; i++) {
        printf("%d ", primes[i]);
    }
    printf("\n\nLast 10 primes: ");
    for (int i = count - 10; i < count; i++) {
        printf("%d ", primes[i]);
    }
    printf("\n");
}

// Verify correctness against known values
int verify_known_count(int limit) {
    // Known prime counts from OEIS A000720
    static const int known[] = {
        0,    // π(1)
        0,    // π(2)
        1,    // π(3)
        3,    // π(5)
        4,    // π(7)
        8,    // π(13)
        10,   // π(17)
        12,   // π(19)
        15,   // π(23)
        168,  // π(1000)
        9592, // π(100000)
        5761455, // π(100000000)
    };
    
    int limit_idx = -1;
    for (int i = 0; i < sizeof(known) / sizeof(known[0]); i++) {
        if (known[i] > limit) {
            limit_idx = i - 1;
            break;
        }
    }
    
    if (limit_idx >= 0) {
        return known[limit_idx];
    }
    return -1;  // No known value
}

int main(int argc, char *argv[]) {
    int limit = LIMIT;
    int start_time, end_time;
    double elapsed;
    
    // Parse command line
    if (argc > 1) {
        limit = atoi(argv[1]);
    }
    
    printf("=== AVX2 Sieve of Eratosthenes ===\n");
    printf("Target limit: %d\n", limit);
    
    // Allocate and initialize
    printf("Initializing sieve (%d bytes)... ", SIEVE_SIZE);
    memset(sieve, 0xff, SIEVE_SIZE);
    printf("done\n");
    
    // Generate base primes
    int base_primes[10000];
    int base_count = 0;
    printf("Generating base primes up to sqrt(%d)... ", limit);
    generate_base_primes(base_primes, &base_count, limit);
    printf("%d primes\n", base_count);
    
    // Sieve with AVX2
    printf("Sieve with AVX2... ");
    clock_t start = clock();
    sieve_primes_avx2(base_primes, base_count);
    clock_t end = clock();
    elapsed = (double)(end - start) / CLOCKS_PER_SEC;
    printf("done (%.3f seconds)\n", elapsed);
    
    // Count primes
    printf("Counting primes... ");
    int count = count_primes(sieve);
    printf("%d primes\n", count);
    
    // Print statistics
    print_stats(limit, count);
    
    // Print sample primes
    print_sample_primes(sieve, count);
    
    // Verify against known value
    int expected = verify_known_count(limit);
    if (expected >= 0) {
        printf("\nVerification (π(%d)):\n", limit);
        if (count == expected) {
            printf("  ✓ Correct! Expected: %d, Got: %d\n", expected, count);
        } else {
            printf("  ✗ ERROR! Expected: %d, Got: %d\n", expected, count);
        }
    }
    
    printf("\nDone!\n");
    
    return 0;
}
