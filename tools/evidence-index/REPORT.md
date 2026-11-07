# Generated evidence report

Levels are exactly those of the coordinator generator. `-` means no capability emitted.
Versions remain exact: evidence never transfers between patches. `E` marks emulation.

| Target | Version | full | cdc | digest | schema_change | selfbuild | dialogue | Execution |
|---|---|---|---|---|---|---|---|---|
| clickhouse | 26.8.21.10 | experimental | - | experimental | - | - | - | - |
| doris | 3.1.4 | experimental | - | experimental | - | - | - | - |
| mariadb-lts | 10.11.19 | experimental | - | - | - | - | - | - |
| mariadb-lts | 10.6.28 | experimental | - | - | - | - | - | - |
| mariadb-lts | 11.4.13 | experimental | - | - | - | - | - | - |
| mariadb-lts | 11.8.9 | experimental | - | - | - | - | - | - |
| mysql57-selfmanaged | 5.7.44 | experimental | - | - | - | - | - | E |
| mysql8-selfmanaged | 8.0.44 | experimental | - | - | - | - | - | - |
| mysql8-selfmanaged | 8.0.46 | designed | - | - | - | - | - | - |
| mysql8-selfmanaged | 8.4.11 | designed | - | - | - | - | - | - |
| mysql8-selfmanaged | 8.4.7 | experimental | - | - | - | - | - | - |
| mysql9-selfmanaged | 9.5.0 | experimental | - | - | - | - | - | - |
| mysql9-selfmanaged | 9.7.2 | designed | - | - | - | - | - | - |
| oceanbase-mysql | 4.3.5.4-104000042025090916 | experimental | - | - | - | - | - | - |
| opengauss-gaussdb | 5.0.3 | designed | - | - | - | - | - | - |
| opengauss-gaussdb | 6.0.0 | designed | - | - | - | - | - | - |
| oracle-11g-xe | 11.2.0.2.0 | experimental | experimental | - | - | - | - | E |
| oracle-21c-xe | 21.3.0.0.0 | experimental | experimental | - | - | - | - | E |
| oracle-23ai-selfmanaged | 23.26.3.0.0 | experimental | experimental | - | - | - | - | - |
| oracle-23ai-selfmanaged | 23.5.0.24.07 | experimental | - | - | - | - | - | - |
| postgres-selfmanaged | 12.22 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 13.23 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 14.23 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 14.24 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 15.18 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 15.19 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 16.14 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 16.15 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 17 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 17.10 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 17.11 | experimental | - | - | - | - | - | - |
| postgres-selfmanaged | 18.4 | designed | - | - | - | - | - | - |
| postgres-selfmanaged | 18.6 | designed | - | - | - | - | - | - |
| sqlserver-selfmanaged | 15.0.4430.1 | verified | experimental | experimental | - | - | - | E |
| sqlserver-selfmanaged | 16.0.4205.1 | verified | experimental | experimental | - | - | - | E |
| sqlserver-selfmanaged | 17.0.4006.2 | verified | experimental | experimental | - | - | - | E |
| starrocks | 3.5.21 | experimental | - | experimental | - | - | - | - |
| tidb | 7.5.0 | experimental | - | - | - | - | - | - |
| tidb | 8.5.0 | experimental | - | - | - | - | - | - |

## S5 P0 gaps

Every observed P0 version is listed, plus untested Oracle 19c. PG 12 is excluded from P0.
The family-only PG 17 failed probe is separate from 17.11. Missing means no row at all;
skip/fail rows are NOT successful acceptance. The final column lists suites with no pass row.

| Target | Version | No evidence row (T0-T8) | No pass row (T0-T8) |
|---|---|---|---|
| mysql57-selfmanaged | 5.7.44 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| mysql8-selfmanaged | 8.0.44 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| mysql8-selfmanaged | 8.0.46 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| mysql8-selfmanaged | 8.4.11 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| mysql8-selfmanaged | 8.4.7 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| mysql9-selfmanaged | 9.5.0 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| mysql9-selfmanaged | 9.7.2 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| oracle-11g-xe | 11.2.0.2.0 | T2, T3, T4, T5, T6 | T0, T2, T3, T4, T5, T6 |
| oracle-19c-selfmanaged | 19c | T0, T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| oracle-21c-xe | 21.3.0.0.0 | T2, T3, T4, T5, T6 | T0, T2, T3, T4, T5, T6 |
| oracle-23ai-selfmanaged | 23.26.3.0.0 | T2, T3, T4, T5, T6 | T0, T1, T2, T3, T4, T5, T6, T8 |
| oracle-23ai-selfmanaged | 23.5.0.24.07 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| postgres-selfmanaged | 13.23 | T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 14.23 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 14.24 | T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 15.18 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 15.19 | T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 16.14 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 16.15 | T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 17 | T0, T1, T2, T3, T4, T5, T6, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 17.10 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 17.11 | T2, T3, T4, T5, T6, T7 | T0, T2, T3, T4, T5, T6, T7 |
| postgres-selfmanaged | 18.4 | T1, T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
| postgres-selfmanaged | 18.6 | T2, T3, T4, T5, T6, T7, T8 | T0, T1, T2, T3, T4, T5, T6, T7, T8 |
