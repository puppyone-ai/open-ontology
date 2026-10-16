DO $target_reader_role_init$
DECLARE
    v_reader CONSTANT text := 'oracle_c5_gateway_reader';
    v_business_role CONSTANT text := 'oo_r_oracle_orders';
    v_membership_count integer;
BEGIN
    IF current_user <> 'oo_target_admin' THEN
        RAISE EXCEPTION 'target role initialization requires oo_target_admin';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles
        WHERE rolname = v_reader
          AND rolcanlogin
          AND NOT rolsuper
          AND NOT rolinherit
          AND NOT rolcreatedb
          AND NOT rolcreaterole
          AND NOT rolreplication
          AND NOT rolbypassrls
    ) THEN
        RAISE EXCEPTION 'reader role is absent or too privileged';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_roles
        WHERE rolname = v_business_role
          AND NOT rolcanlogin
          AND NOT rolsuper
          AND NOT rolcreatedb
          AND NOT rolcreaterole
          AND NOT rolreplication
          AND NOT rolbypassrls
    ) THEN
        RAISE EXCEPTION 'business role is absent or too privileged';
    END IF;

    IF has_database_privilege(v_reader, current_database(), 'CONNECT') IS NOT TRUE THEN
        RAISE EXCEPTION 'reader cannot connect to target database';
    END IF;

    SELECT count(*)
    INTO v_membership_count
    FROM pg_catalog.pg_auth_members m
    JOIN pg_catalog.pg_roles member ON member.oid = m.member
    WHERE member.rolname = v_reader;

    IF v_membership_count = 0 THEN
        EXECUTE format(
            'GRANT %I TO %I WITH ADMIN FALSE, INHERIT FALSE, SET TRUE',
            v_business_role,
            v_reader
        );
    ELSIF v_membership_count = 1 AND EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members m
        JOIN pg_catalog.pg_roles member ON member.oid = m.member
        JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid
        WHERE member.rolname = v_reader
          AND granted.rolname = v_business_role
          AND NOT m.admin_option
          AND NOT m.inherit_option
          AND m.set_option
    ) THEN
        NULL;
    ELSE
        RAISE EXCEPTION 'reader has unexpected role memberships';
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_auth_members m
        JOIN pg_catalog.pg_roles member ON member.oid = m.member
        JOIN pg_catalog.pg_roles granted ON granted.oid = m.roleid
        WHERE member.rolname = v_reader
          AND granted.rolname = v_business_role
          AND NOT m.admin_option
          AND NOT m.inherit_option
          AND m.set_option
    ) OR pg_catalog.pg_has_role(v_reader, v_business_role, 'SET') IS NOT TRUE THEN
        RAISE EXCEPTION 'reader role membership did not verify';
    END IF;

    RAISE NOTICE 'target_reader_role_init=verified';
END
$target_reader_role_init$;
