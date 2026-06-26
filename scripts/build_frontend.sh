#!/usr/bin/env bash
set -e

echo "=== Building Frontend Dist ==="

cd frontend

# Run the python build script to generate static HTML from views
python3 build.py

echo "=== Frontend Build Complete ==="
