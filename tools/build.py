#!/usr/bin/env python3
import os
import sys

if __name__ == "__main__":
    tools_builder = os.path.join(os.path.dirname(__file__), "tools", "builder.py")
    os.execv(sys.executable, [sys.executable, tools_builder] + sys.argv[1:])
