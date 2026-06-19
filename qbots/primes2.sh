#!/bin/bash
# Calculate primes up to 10 million using Sieve of Eratosthenes

LIMIT=10000000

# Create array and mark all as potential primes
declare -a is_prime
for ((i=0; i<=LIMIT; i++)); do
    is_prime[$i]=1
done

# 0 and 1 are not primes
is_prime[0]=0
is_prime[1]=0

# Sieve of Eratosthenes
for ((i=2; i*i<=LIMIT; i++)); do
    if [[ ${is_prime[$i]} -eq 1 ]]; then
        # Mark multiples starting from i*i
        for ((j=i*i; j<=LIMIT; j+=i)); do
            is_prime[$j]=0
        done
    fi
done

# Output primes
for ((i=2; i<=LIMIT; i++)); do
    if [[ ${is_prime[$i]} -eq 1 ]]; then
        echo $i
    fi
done
