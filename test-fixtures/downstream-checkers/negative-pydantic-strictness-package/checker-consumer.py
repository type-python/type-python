from app import StrictUser


good_user = StrictUser(user_id=1, name="Ada")
bad_user = StrictUser(user_id=1, name="Ada", unexpected=True)
