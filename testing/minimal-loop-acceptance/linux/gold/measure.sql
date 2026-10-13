-- Oracle read of the sealed bulk shadows. Not an agent prompt.
\pset tuples_only on
\pset format unaligned
SET statement_timeout = '30s';
SELECT 'reader=' || current_user;
SELECT 'database=' || current_database();
SELECT 'server_version=' || current_setting('server_version');
SELECT 'count_batch_c=' || count(*)::text
FROM "_oo"."pt_591bac24fd79b573769b0cf3e96d688bb618ce2f6c524a6ee7222a548e45";
SELECT 'id_overlap=' || count(*)::text
FROM "_oo"."pt_0c7aa3eae81470fae5787d6aadfb6d3730c81196eba85b9b4bee0d724760" AS a
JOIN "_oo"."pt_591bac24fd79b573769b0cf3e96d688bb618ce2f6c524a6ee7222a548e45" AS c
  ON a."pc_3033fb289eea24eb9858008870613679eb9f35192d149a4a133c03c6bde4"
   = c."pc_1320025b729845187a13ec2bf9c38f0ac85c92ae954d8d37cf63abe42ccd";
SELECT 'amount_sum_qty_above_2=' || COALESCE(sum(c."pc_677f61a4cab707cca457e2771ecb402200e1ff86aeabaa04fc267ccf4f3d")::text, 'null')
FROM "_oo"."pt_591bac24fd79b573769b0cf3e96d688bb618ce2f6c524a6ee7222a548e45" AS c
WHERE c."pc_4cddd641b7080f7e54bf91940c93ca5a801c910db7652803fc16025494dd" > 2;
SELECT 'null_amount_a=' || count(*)::text
FROM "_oo"."pt_0c7aa3eae81470fae5787d6aadfb6d3730c81196eba85b9b4bee0d724760"
WHERE "pc_849044d2bdc902504dc35e884c5d418e48f23dc2f877bbff5448eb8c459e" IS NULL;
SELECT 'amount_a_id_2=' || COALESCE("pc_849044d2bdc902504dc35e884c5d418e48f23dc2f877bbff5448eb8c459e"::text, 'null')
FROM "_oo"."pt_0c7aa3eae81470fae5787d6aadfb6d3730c81196eba85b9b4bee0d724760"
WHERE "pc_3033fb289eea24eb9858008870613679eb9f35192d149a4a133c03c6bde4" = 2;
SELECT 'qty_c_id_1=' || "pc_4cddd641b7080f7e54bf91940c93ca5a801c910db7652803fc16025494dd"::text
FROM "_oo"."pt_591bac24fd79b573769b0cf3e96d688bb618ce2f6c524a6ee7222a548e45"
WHERE "pc_1320025b729845187a13ec2bf9c38f0ac85c92ae954d8d37cf63abe42ccd" = 1;
SELECT 'qty_c_id_2=' || "pc_4cddd641b7080f7e54bf91940c93ca5a801c910db7652803fc16025494dd"::text
FROM "_oo"."pt_591bac24fd79b573769b0cf3e96d688bb618ce2f6c524a6ee7222a548e45"
WHERE "pc_1320025b729845187a13ec2bf9c38f0ac85c92ae954d8d37cf63abe42ccd" = 2;
