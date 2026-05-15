import os

# Read from input.txt in the current directory
with open("input.txt", "r") as f:
    data = f.read()

# Write to output.txt
with open("output.txt", "w") as f:
    f.write(data.upper())

print(f"Processed {len(data)} characters.")
