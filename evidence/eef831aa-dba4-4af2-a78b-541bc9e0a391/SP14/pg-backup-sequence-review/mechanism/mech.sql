--
-- PostgreSQL database dump
--

\restrict ka2excuLB5UY6skGGDn4O9wneHAmD2g0SyNzHpLMmdBtTTtRqHVoE9WbPOkpNYl

-- Dumped from database version 17.10 (Debian 17.10-1.pgdg12+1)
-- Dumped by pg_dump version 17.10 (Debian 17.10-1.pgdg12+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: mech; Type: SCHEMA; Schema: -; Owner: oo_a391_owner
--

CREATE SCHEMA mech;


ALTER SCHEMA mech OWNER TO oo_a391_owner;

--
-- Name: cached; Type: SEQUENCE; Schema: mech; Owner: oo_a391_owner
--

CREATE SEQUENCE mech.cached
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 10;


ALTER SEQUENCE mech.cached OWNER TO oo_a391_owner;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: ident_row; Type: TABLE; Schema: mech; Owner: oo_a391_owner
--

CREATE TABLE mech.ident_row (
    id bigint NOT NULL
);


ALTER TABLE mech.ident_row OWNER TO oo_a391_owner;

--
-- Name: ident_row_id_seq; Type: SEQUENCE; Schema: mech; Owner: oo_a391_owner
--

ALTER TABLE mech.ident_row ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME mech.ident_row_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: item; Type: TABLE; Schema: mech; Owner: oo_a391_owner
--

CREATE TABLE mech.item (
    id bigint NOT NULL
);


ALTER TABLE mech.item OWNER TO oo_a391_owner;

--
-- Name: owned; Type: SEQUENCE; Schema: mech; Owner: oo_a391_owner
--

CREATE SEQUENCE mech.owned
    START WITH 41
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE mech.owned OWNER TO oo_a391_owner;

--
-- Name: owned; Type: SEQUENCE OWNED BY; Schema: mech; Owner: oo_a391_owner
--

ALTER SEQUENCE mech.owned OWNED BY mech.item.id;


--
-- Name: plain; Type: SEQUENCE; Schema: mech; Owner: oo_a391_owner
--

CREATE SEQUENCE mech.plain
    START WITH 41
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE mech.plain OWNER TO oo_a391_owner;

--
-- Data for Name: ident_row; Type: TABLE DATA; Schema: mech; Owner: oo_a391_owner
--

COPY mech.ident_row (id) FROM stdin;
\.


--
-- Data for Name: item; Type: TABLE DATA; Schema: mech; Owner: oo_a391_owner
--

COPY mech.item (id) FROM stdin;
\.


--
-- Name: cached; Type: SEQUENCE SET; Schema: mech; Owner: oo_a391_owner
--

SELECT pg_catalog.setval('mech.cached', 20, true);


--
-- Name: ident_row_id_seq; Type: SEQUENCE SET; Schema: mech; Owner: oo_a391_owner
--

SELECT pg_catalog.setval('mech.ident_row_id_seq', 1, false);


--
-- Name: owned; Type: SEQUENCE SET; Schema: mech; Owner: oo_a391_owner
--

SELECT pg_catalog.setval('mech.owned', 41, false);


--
-- Name: plain; Type: SEQUENCE SET; Schema: mech; Owner: oo_a391_owner
--

SELECT pg_catalog.setval('mech.plain', 41, false);


--
-- Name: item item_pkey; Type: CONSTRAINT; Schema: mech; Owner: oo_a391_owner
--

ALTER TABLE ONLY mech.item
    ADD CONSTRAINT item_pkey PRIMARY KEY (id);


--
-- Name: SEQUENCE plain; Type: ACL; Schema: mech; Owner: oo_a391_owner
--

GRANT USAGE,UPDATE ON SEQUENCE mech.plain TO oo_a391_stranger;


--
-- PostgreSQL database dump complete
--

\unrestrict ka2excuLB5UY6skGGDn4O9wneHAmD2g0SyNzHpLMmdBtTTtRqHVoE9WbPOkpNYl

