import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

# Starlette's TestClient sends "Host: testserver"; the engine answers only
# 127.0.0.1 and localhost unless told otherwise (app/security.py).
os.environ.setdefault("SIMSTUDIO_ALLOWED_HOSTS", "testserver")
