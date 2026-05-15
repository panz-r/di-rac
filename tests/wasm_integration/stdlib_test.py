import json
import sys
import os

print(json.dumps({
    "status": "ok",
    "version": sys.version,
    "cwd": os.getcwd()
}))
