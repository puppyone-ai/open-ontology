\ir prelude.sql
SELECT session_user::text || '|' || current_user::text;
