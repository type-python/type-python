class RuntimeUser:
    def __init__(self, name: str) -> None:
        self.name = name


def runtime_label(name: str) -> str:
    return f"runtime:{name}"
