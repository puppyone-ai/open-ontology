import json
import os
import tempfile
import unittest
from pathlib import Path
from customer_input import CustomerRefusal, load_customer

class CustomerInputTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.secret = self.root/'secret'
        self.secret.write_text('not-an-actual-credential')
        self.secret.chmod(0o600)
        self.registry = self.root/'registry.json'
        self.registry.write_text(json.dumps({'secret://example/reader': str(self.secret)}))
        self.input = self.root/'input.json'
        self.doc = {'protocol':'oo.customer-input/v1', 'secret_ref':'secret://example/reader',
                    'source':{'reader':'CUSTOMER_READ','service':'SALES','container':'sales.example'},
                    'approved_objects':['SALES.ORDERS'],
                    'selections':[{'object':'sales.orders','profile':'batch','fields':['ID','N16','N1','D','C6','V2','R']}],
                    'budget':{'unit':'Rows','hard_limit':8}}
    def tearDown(self): self.tmp.cleanup()
    def load(self):
        self.input.write_text(json.dumps(self.doc))
        return load_customer(self.input, self.registry)
    def test_other_customer_object_and_reference(self):
        loaded = self.load()
        self.assertEqual(loaded['selections'][0]['object'],'SALES.ORDERS')
        steps = [item['step'] for item in loaded['_open_trace']]
        self.assertEqual(steps, ['protocol', 'source_identifiers', 'authorization', 'registry_before_open', 'registry_open', 'secret_file'])
        self.assertFalse(loaded['_open_trace'][3]['registry_io'])
        self.assertTrue(loaded['_open_trace'][4]['registry_io'])
        self.assertFalse(loaded['_open_trace'][5]['secret_bytes_recorded'])
        self.assertEqual(loaded['_open_trace'][5]['mode'], '0600')
    def test_unknown_object_refused_before_registry(self):
        self.registry.unlink()
        self.doc['selections'][0]['object']='SALES.SECRET'
        with self.assertRaisesRegex(ValueError,'unapproved object'): self.load()
    def test_refusal_trace_does_not_open_registry(self):
        missing = self.root/'never-opened.json'
        self.doc['selections'][0]['object']='SALES.SECRET'
        self.input.write_text(json.dumps(self.doc))
        with self.assertRaises(CustomerRefusal) as caught:
            load_customer(self.input, missing)
        self.assertFalse(missing.exists())
        step = caught.exception.trace[-1]
        self.assertEqual(step['step'], 'registry_before_open')
        self.assertFalse(step['registry_io'])
        self.assertFalse(step['password_read'])
        self.assertFalse(step['source_io'])
        self.assertFalse(step['oracle_read'])
    def test_unknown_field_refused_before_registry(self):
        self.registry.unlink()
        self.doc['selections'][0]['fields']=['PASSWORD']
        with self.assertRaisesRegex(ValueError,'field'): self.load()
    def test_sql_identifier_injection(self):
        self.doc['approved_objects']=['SALES.ORDERS;DROP TABLE X']
        with self.assertRaises(ValueError): self.load()
    def test_substring_secret_reference_not_resolved(self):
        self.doc['secret_ref']='secret://example/reader-other'
        with self.assertRaisesRegex(ValueError,'unresolved'): self.load()
    def test_world_readable_secret_refused(self):
        self.secret.chmod(0o644)
        with self.assertRaisesRegex(ValueError,'mode'): self.load()
    def test_symlink_secret_refused(self):
        link=self.root/'link';link.symlink_to(self.secret)
        self.registry.write_text(json.dumps({'secret://example/reader':str(link)}))
        with self.assertRaises(ValueError): self.load()
    def test_boolean_budget_refused(self):
        self.doc['budget']['hard_limit']=True
        with self.assertRaises(ValueError): self.load()

if __name__=='__main__': unittest.main()
