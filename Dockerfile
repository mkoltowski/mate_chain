FROM python:3.9-slim
# To wymusi natychmiastowe wypisywanie logów:
ENV PYTHONUNBUFFERED=1 
RUN pip install pycryptodome
COPY symulator.py .
CMD ["python", "symulator.py"]