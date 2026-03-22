class timedelta:
    pass

class tzinfo:
    pass

class date:
    @classmethod
    def today(cls) -> date: ...

class datetime(date):
    @classmethod
    def now(cls) -> datetime: ...
    @classmethod
    def utcnow(cls) -> datetime: ...
