from pyModbusTCP.client import ModbusClient
client = ModbusClient(host="127.0.0.1", port=5502, auto_open=True)
print("--- TEST PROTOKOŁU MODBUS TCP ---")
rejestry = client.read_input_registers(0, 3)
if rejestry:
    print("✓ Połączenie udane. Pobrane rejestry:")
    print(f"  Wtryskarka 1: {rejestry[0] / 10.0} °C")
    print(f"  Wtryskarka 2: {rejestry[1] / 10.0} °C")
    print(f"  Wtryskarka 3: {rejestry[2] / 10.0} °C")
else:
    print("✗ Błąd połączenia z bramą Modbus")
