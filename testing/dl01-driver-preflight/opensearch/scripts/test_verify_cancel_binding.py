import copy
import importlib.util
import unittest
from pathlib import Path
spec = importlib.util.spec_from_file_location('binding', Path(__file__).with_name('verify_cancel_binding.py'))
binding = importlib.util.module_from_spec(spec)
spec.loader.exec_module(binding)
class BindingTests(unittest.TestCase):
    def setUp(self):
        self.line = '2026-10-10T23:37:50.333462553Z shardFailures {[NODE][approved][0]: TaskCancelledException[cancelled task with reason: channel closed]; }'
        self.doc = {'task_before': {'task_id':'NODE:308','description':'marker','cancelled':False},'task_after':{'task_id':'NODE:308','description':'marker','cancelled':True,'cancellation_time_millis':1791675469724},'user_triggered_unix_ms':1791675469721,'workload_index':'approved','marker':'marker','task_id_present_after':False,'task_list_after_status':200,'other_search_task_ids_before':[],'natural_completion_before_trigger':False,'cancel_after_time_interval_sent':False,'dba_cancel_used':False}
        self.doc['server_log_binding']={'line':self.line,'task_id':'NODE:308','index':'approved','marker':'marker','trigger_unix_ms':1791675469721,'cancelled_unix_ms':1791675469724}
    def test_current_owned_native_log(self):
        self.assertEqual(binding.verify(self.doc,self.line), [])
    def test_missing_raw_line(self):
        self.assertTrue(binding.verify(self.doc,''))
    def test_wrong_task(self):
        self.doc['task_after']['task_id']='NODE:309'
        self.assertTrue(binding.verify(self.doc,self.line))
    def test_still_running(self):
        self.doc['task_id_present_after']=True
        self.assertTrue(binding.verify(self.doc,self.line))
    def test_other_search(self):
        self.doc['other_search_task_ids_before']=['NODE:309']
        self.assertTrue(binding.verify(self.doc,self.line))
    def test_stale_time(self):
        self.doc['user_triggered_unix_ms']+=10000
        self.assertTrue(binding.verify(self.doc,self.line))
    def test_deadline(self):
        self.doc['cancel_after_time_interval_sent']=True
        self.assertTrue(binding.verify(self.doc,self.line))
    def test_late(self):
        self.doc['natural_completion_before_trigger']=True
        self.assertTrue(binding.verify(self.doc,self.line))
if __name__ == '__main__': unittest.main()
