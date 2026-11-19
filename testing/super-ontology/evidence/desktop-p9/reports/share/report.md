# Super Drive share and mount (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| the scope lists the added path | pass |  |
| the defaults are a suggester, 7 days and 800 tokens | pass |  |
| the preview shows an AI-generated badge | pass |  |
| the BRIEF has source marks | pass | 10 marks |
| the preview says how many items were held back as a number only | pass |  |
| the command line carries the folder, the scope and the task | pass | oo context mount --drive drive --scope Projects --to /private/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-share-8KQngT/tasks/proj-a --for 'Follow up the website redesign' --budget 800 --role suggester --expires 7d |
| the engine has an active share for it | pass | active |
| the folder has a BRIEF and a manifest | pass | BRIEF.md, INDEX.md, SHARE.json, decisions.md, manifest.json, people/吴启明-86d3ab06.md, people/周若溪-9031b192.md, people/孙晓棠-0fcf5be9.md, people/林川-5666dae9.md, people/贺文博-5d203e93.md, people/郑雨桐-e3158dc1.md, people/陈子墨-67466186.md, people/韩立-a6142c2b.md, projects/a-6e5476ca.md, projects/官网改版-b5d0f18e.md, projects/星图看板-e867af93.md, projects/灯塔迁移-9dbbeb8a.md, projects/知了客服-bd8a9376.md, refs/0001.md, refs/0002.md, refs/0003.md, refs/0004.md, refs/0005.md, refs/0006.md, refs/0007.md, refs/0008.md, refs/0009.md, refs/0010.md, refs/0011.md, refs/0012.md, refs/0013.md, refs/0014.md, refs/0015.md, refs/0016.md, refs/0017.md, refs/0018.md, refs/0019.md, refs/0020.md, refs/0021.md, refs/0022.md, suggest/README.md, terms.md |
| the one-click mount and the CLI write the same files, byte for byte | pass | 42 files |
| every [src:...] of the BRIEF opens in the app on its block | pass | [] |
| the confirmation says read content can't be taken back | pass |  |
| the engine shows the share as revoked | pass | revoked |
| the .context folder is gone | pass |  |

Timings: {}

