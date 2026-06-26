"""Square number generation module.

Generates square numbers for a given range of integers and outputs
the results as a JSON file. Follows the same coding style conventions
as the fibonacci module.
"""

import json


def generate_squares(start: int, end: int) -> list[dict[str, int]]:
    """Generate square numbers for all integers in the range [start, end].

    Args:
        start: The starting integer of the range (inclusive).
        end: The ending integer of the range (inclusive).

    Returns:
        A list of dictionaries, each containing the original number
        and its square: [{"number": n, "square": n**2}, ...].

    Raises:
        ValueError: If start > end.
    """
    if start > end:
        raise ValueError(f"start ({start}) must not exceed end ({end})")

    return [{"number": n, "square": n ** 2} for n in range(start, end + 1)]


def save_to_json(data: list[dict[str, int]], filename: str) -> None:
    """Save a list of dictionaries to a JSON file with pretty formatting.

    Args:
        data: The data to save as a list of dictionaries.
        filename: The output JSON file path.

    Raises:
        IOError: If the file cannot be written.
    """
    with open(filename, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)
    print(f"Results saved to {filename}")


def main() -> None:
    """CLI entry point for generating square numbers.

    Generates squares for numbers 1 through 10 and saves the results
    to a JSON file named 'output.json'.
    """
    start, end = 1, 10

    try:
        result = generate_squares(start, end)
        save_to_json(result, "output.json")
        print(f"Generated squares for numbers {start} to {end}:")
        for item in result:
            print(f"  {item['number']}^2 = {item['square']}")
    except (ValueError, IOError) as e:
        print(f"Error: {e}")
        import sys
        sys.exit(1)


if __name__ == "__main__":
    main()
