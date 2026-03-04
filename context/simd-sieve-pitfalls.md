# SIMD Sieve Implementation Pitfalls

This document captures critical pitfalls encountered during SIMD-accelerated sieve implementation.

## 1. Memory Alignment Issues

### Problem: `_mm256_load_si256` requires 32-byte alignment

**Error:**
```c
// WRONG: May cause segmentation fault on misaligned data
__m256i block = _mm256_load_si256((__m256i*)sieve);
```

**Solution:**
```c
// Option 1: Use aligned allocation
alignas(32) uint8_t sieve[SIEVE_SIZE];

// Option 2: Use unaligned load (slower)
__m256i block = _mm256_loadu_si256((__m256i*)sieve);

// Option 3: Manual alignment
size_t offset = (size_t)sieve & 31;
if (offset) {
    // Process offset bytes with scalar
    sieve += offset;
}
```

**Detection:**
```bash
# Check alignment in assembly
objdump -d ./sieve | grep -A10 "_mm256"
```

---

## 2. Mask Overflow

### Problem: Mask bits exceed vector width

**Error:**
```c
// WRONG: stride=50 exceeds 32-bit mask
__mmask32 mask = generate_stride_mask(50);  // Undefined behavior!
```

**Solution:**
```c
// Ensure stride is within bounds
static inline __mmask32 generate_stride_mask(int stride) {
    if (stride >= 32) return 0;  // No valid marks
    
    __mmask32 mask = 0;
    for (int i = 0; i < 32; i++) {
        if (i % stride == 0) mask |= (1 << i);
    }
    return mask;
}
```

**Detection:**
```bash
# Add bounds checking
if (stride >= 32) {
    fprintf(stderr, "Error: stride %d exceeds mask width\n", stride);
}
```

---

## 3. Boundary Conditions

### Problem: Partial blocks at end of sieve

**Error:**
```c
// WRONG: May access beyond sieve bounds
for (int i = 0; i < NUM_CANDIDATES; i += 32) {
    __m256i block = _mm256_load_si256((__m256i*)(sieve + i));
    // May read past end of array!
}
```

**Solution:**
```c
// Process full blocks, then handle remainder
int full_blocks = NUM_CANDIDATES / BLOCK_SIZE;
int remainder = NUM_CANDIDATES % BLOCK_SIZE;

for (int i = 0; i < full_blocks; i++) {
    // Process full block
}

// Handle remainder with scalar code
for (int i = full_blocks * BLOCK_SIZE; i < NUM_CANDIDATES; i++) {
    // Scalar marking
}
```

---

## 4. False Sharing in Multi-threaded Sieve

### Problem: Multiple threads write adjacent cache lines

**Error:**
```c
// WRONG: Thread 0 writes bytes 0-63, thread 1 writes bytes 64-127
// Both affect same cache line!
void thread_sieve(int thread_id, int *primes) {
    sieve[thread_id * SEGMENT_SIZE] = 0;  // False sharing!
}
```

**Solution:**
```c
// Pad to cache line boundaries
#define CACHE_LINE_PAD 64
#define THREAD_LOCAL_SIZE (SEGMENT_SIZE + CACHE_LINE_PAD)

// Each thread gets own cache-aligned region
void thread_sieve(int thread_id, int *primes) {
    uint8_t *local_sieve = thread_buffers[thread_id];
    // No false sharing
}
```

---

## 5. Branch Prediction

### Problem: Conditional marking introduces mispredicts

**Error:**
```c
// WRONG: Branch inside tight loop
for (int i = 0; i < BLOCK_SIZE; i++) {
    if (i % stride == 0) {
        block[i] = 0;  // Branch mispredict!
    }
}
```

**Solution:**
```c
// Use mask-based operations
__mmask32 mask = generate_stride_mask(stride);
block = _mm256_andnot_si256(block, _mm256_set_m32(mask));
// No branches!
```

---

## 6. Data Dependency

### Problem: Sequential marking creates dependencies

**Error:**
```c
// WRONG: Each iteration depends on previous
for (int i = 0; i < NUM_CANDIDATES; i++) {
    if (is_bit_set(sieve, i)) {
        // Mark multiples
        for (int j = i * 2; j < NUM_CANDIDATES; j += i) {
            clear_bit(sieve, j);  // Depends on sieve state!
        }
    }
}
```

**Solution:**
```c
// Process all primes in parallel
for (int prime : base_primes) {
    sieve_block_avx2(sieve, 0, NUM_CANDIDATES, prime_stride);
}
// No dependencies between iterations!
```

---

## 7. Endianness Issues

### Problem: Bit order differs between little/big endian

**Error:**
```c
// WRONG: Assumes little-endian bit order
int bit = 0;
uint8_t byte = sieve[0];
__m256i block = _mm256_set_epi8(byte, ...);  // Bit order matters!
```

**Solution:**
```c
// Use consistent bit ordering
// Intel intrinsics use little-endian by default
// Document this assumption clearly!
```

---

## 8. Compiler Optimizations

### Problem: Missing `-mavx2` flag

**Error:**
```bash
# WRONG: No AVX2 instructions generated
gcc -O3 sieve.c -o sieve
```

**Solution:**
```bash
# Correct: Enable AVX2
gcc -O3 -mavx2 -mtune=native sieve.c -o sieve
```

**Detection:**
```bash
# Check for AVX2 instructions
objdump -d ./sieve | grep -c "_mm256"
# Should be > 0 if AVX2 used
```

---

## 9. Cache Line Conflicts

### Problem: Strided marking evicts cache lines

**Error:**
```c
// WRONG: Marking multiples of 3 accesses every 4th byte
// Causes cache line thrashing
for (int j = 3; j < LIMIT; j += 3) {
    sieve[j] = 0;
}
```

**Solution:**
```c
// Process cache-line aligned blocks
for (int block = 0; block < NUM_BLOCKS; block++) {
    uint8_t *block_start = sieve + block * CACHE_LINE_SIZE;
    // Mark all multiples within this cache line
    // Minimize cache line evictions
}
```

---

## 10. Integer Overflow

### Problem: `i * i` overflows for large `i`

**Error:**
```c
// WRONG: i*i overflows when i > 46340
for (int i = 2; i * i <= LIMIT; i++) {
    // i*i may overflow!
}
```

**Solution:**
```c
// Use proper type
for (uint32_t i = 2; i * i <= LIMIT; i++) {
    // Safe for LIMIT < 2^32
}
```

---

## Testing Checklist

- [ ] Verify alignment with `alignof(sieve)`
- [ ] Test with small limits (100, 1000, 10000)
- [ ] Verify against known prime counts (OEIS A000720)
- [ ] Check for memory leaks with Valgrind
- [ ] Profile with `perf` for cache misses
- [ ] Verify AVX2 instructions with `objdump`
- [ ] Test multi-threaded version for false sharing
- [ ] Check endianness on target platform

---

## Debugging Tips

### 1. Check SIMD Usage
```bash
objdump -d ./sieve | grep -A5 "_mm256"
```

### 2. Profile Cache Behavior
```bash
perf record -e cache-misses,cache-references ./sieve
perf report
```

### 3. Memory Layout
```bash
objdump -s -j .data ./sieve | head -50
```

### 4. Valgrind for Memory Errors
```bash
valgrind --tool=memcheck --leak-check=full ./sieve
```

---

**Last Updated:** March 2026
**Contributors:** AI Assistant (Slop Project)
