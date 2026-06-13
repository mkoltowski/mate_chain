FROM python:3.12-slim

ENV PYTHONUNBUFFERED=1

RUN pip install --no-cache-dir pycryptodome

WORKDIR /app
COPY symulator.py .

CMD ["python", "symulator.py"]