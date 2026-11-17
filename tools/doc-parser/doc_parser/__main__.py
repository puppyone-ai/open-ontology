from .config import Config
from .server import serve

if __name__ == "__main__":
    serve(Config.from_env())
