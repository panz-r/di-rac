import sys

# Test large stdout (1MB)
chunk = "A" * 1024
for _ in range(1024):
    sys.stdout.write(chunk)
sys.stdout.flush()

# Test large stderr (1MB)
for _ in range(1024):
    sys.stderr.write(chunk)
sys.stderr.flush()
