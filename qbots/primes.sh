#!/bin/bash
# Sieve of Eratosthenes - calculate primes up to 10 million

LIMIT=10000000

# Initialize array (0 = prime candidate, 1 = composite)
declare -a is_prime
for ((i=0; i<=LIMIT; i++)); do
    is_prime[$i]=0
done

# 0 and 1 are not primes
is_prime[0]=1
is_prime[1]=1

# Sieve of Eratosthenes
for ((i=2; i*i<=LIMIT; i++)); do
    if [[ ${is_prime[$i]} -eq 0 ]]; then
        # Mark multiples of i as composite
        for ((j=i*i; j<=LIMIT; j+=i)); do
            is_prime[$j]=1
        done
    fi
done

# Count and print primes
count=0
for ((i=2; i<=LIMIT; i++)); do
    if [[ ${is_prime[$i]} -eq 0 ]]; then
        ((count++))
        echo $i
    fi
done

echo "Total primes found: $count" >&2
