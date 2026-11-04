"""SQL Server dialect over H's neutral row/shape generator."""
from pathlib import Path


def generate(common, scale, tables):
    source = iter(common.generate("mysql", scale=scale, tables=tables))
    next(source)  # Only the dialect preamble is replaced; neutral rows stay shared.
    yield (Path(__file__).resolve().parents[2] /
           "testing/engines/sqlserver/seed/preamble.sql").read_text()
    for statement in source:
        statement = statement.replace("BIGINT AUTO_INCREMENT", "BIGINT IDENTITY(1000,1)")
        statement = statement.replace("VARCHAR(100)", "NVARCHAR(100)")
        statement = statement.replace("TIMESTAMP", "DATETIME2(6)")
        statement = statement.replace("DATE '", "'")
        statement = statement.replace("REPEAT('x',16000)", "REPLICATE(CAST(N'x' AS nvarchar(max)),16000)")
        statement = statement.replace("MOD(n-1,120)", "((n-1)%120)").replace("MOD(n,7)", "(n%7)")
        if statement.startswith("INSERT INTO regions"):
            statement = statement.replace("INSERT INTO regions VALUES", "INSERT INTO regions (id,name) VALUES")
            statement = "SET IDENTITY_INSERT regions ON;\n" + statement + "SET IDENTITY_INSERT regions OFF;\n"
        if statement.startswith("INSERT INTO string_keys"):
            statement = statement.replace("VALUES ('", "VALUES (N'")
        statement = statement.replace("DROP TABLE seed_digits;\nCREATE VIEW",
                                      "DROP TABLE seed_digits;\nGO\nCREATE VIEW")
        if statement == "COMMIT;\n":
            statement = "GO\n"
        yield statement
