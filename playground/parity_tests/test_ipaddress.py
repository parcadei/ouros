import ipaddress

# === v4_int_to_packed ===
try:
    print('v4_int_to_packed_default', ipaddress.v4_int_to_packed(0))
    print('v4_int_to_packed_combo_req_2', ipaddress.v4_int_to_packed(1))
except Exception as e:
    print('SKIP_v4_int_to_packed', type(e).__name__, e)

# === v6_int_to_packed ===
try:
    print('v6_int_to_packed_default', ipaddress.v6_int_to_packed(0))
    print('v6_int_to_packed_combo_req_2', ipaddress.v6_int_to_packed(1))
except Exception as e:
    print('SKIP_v6_int_to_packed', type(e).__name__, e)
