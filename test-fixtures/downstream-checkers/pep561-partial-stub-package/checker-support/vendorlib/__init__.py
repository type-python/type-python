from vendorlib.runtime import RuntimeUser


def lookup_user(user_id: int):
    return RuntimeUser(f"user-{user_id}")
