#!/bin/bash
set -e

echo "Building Rust backend..."
cargo build --release --bin api
cp target/release/api deploy/

echo "Building frontend..."
cd frontend
npm run build
cp -r dist/* ../deploy/frontend/
cd ..

echo "Setting permissions..."
chmod +x deploy/api
chmod -R 755 deploy/frontend/

echo "Build complete!"
echo "Deploy directory contents:"
ls -la deploy/
