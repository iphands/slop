FROM python:3.11-slim

# Set working directory
WORKDIR /app

# Install system dependencies for cron
RUN apt-get update && apt-get install -y cron && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY main.py .
COPY commands/ ./commands/

RUN useradd -m -u 1000 appuser && chown -R appuser:appuser /app
USER appuser

COPY crontab /etc/cron.d/plug
RUN chmod 0644 /etc/cron.d/plug

RUN echo '#!/bin/bash
python /app/main.py plug $1 >> /var/log/plug.log 2>&1' > /app/run_plug.sh && chmod +x /app/run_plug.sh

# Start cron daemon
cmd ["cron", "-f", "-l"]
