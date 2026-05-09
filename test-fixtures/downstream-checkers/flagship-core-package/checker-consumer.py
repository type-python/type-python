from __future__ import annotations

from app import (
    Client,
    Found,
    LookupResult,
    PublicUser,
    ReadonlyUserRecord,
    User,
    apply_patch,
    clean_label,
    describe_result,
    freeze_record,
    lookup_user,
    public_user,
    render_all,
)


client = Client()
record = client.get_user(1)
public: PublicUser = public_user(record)
readonly: ReadonlyUserRecord = freeze_record(record)
result: LookupResult = Found(public)
message: str = describe_result(result)
clean_name: str = clean_label(" Ada ")
patched = apply_patch(record, {"name": "Grace"})
rendered: list[str] = render_all([User(readonly["id"], readonly["name"], readonly["status"])])

assert message
assert clean_name == "Ada"
assert patched["name"] == "Grace"
assert rendered
assert lookup_user(client, -1)
