"""Fibonacci number computation module.

Provides functions to compute Fibonacci numbers with input validation
and a CLI interface for command-line usage.
"""


def fibonacci(n: int) -> int:
    """Compute the nth Fibonacci number using an iterative approach.

    Args:
        n: A non-negative integer indicating which Fibonacci number to compute.
           n=0 returns 0, n=1 returns 1, n=2 returns 1, etc.

    Returns:
        The nth Fibonacci number as an integer.

    Raises:
        ValueError: If n is negative.
    """
    if n < 0:
        raise ValueError(f"n must be a non-negative integer, got {n}")

    if n <= 1:
        return n

    a, b = 0, 1
    for _ in range(2, n + 1):
        a, b = b, a + b
    return b


def main() -> None:
    """CLI entry point for computing Fibonacci numbers.

    Reads an integer from command-line arguments and prints
    the corresponding Fibonacci number.
    """
    import sys

    if len(sys.argv) != 2:
        print("Usage: python fibonacci.py <n>")
        print("Example: python fibonacci.py 10")
        sys.exit(1)

    try:
        n = int(sys.argv[1])
        result = fibonacci(n)
        print(f"fibonacci({n}) = {result}")
    except ValueError as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
