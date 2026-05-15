import os
import sys

print(f"ENV_VAR: {os.environ.get('TEST_VAR', 'NOT_FOUND')}")
print(f"GUEST_ARG: {sys.argv[1] if len(sys.argv) > 1 else 'NONE'}")
