from app import fetch_user_name
from vendorlib.runtime import runtime_label


name: str = fetch_user_name()
label: str = runtime_label("Ada")
