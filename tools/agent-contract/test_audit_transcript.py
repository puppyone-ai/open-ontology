import unittest

from audit_transcript import audit


class TranscriptAuditTests(unittest.TestCase):
    def test_fenced_headings_are_not_questions_and_nested_json_is_not_recounted(self):
        text = '''## 问题 2
#### 1 tool `terminal`（距上一条 1 秒）
````text
exit_code: 0
## 问题 99
{
  "error": {"code": "INTERNAL", "message": "refused"},
  "query_id": "q_failed"
}
{"query_id":"q_ok","rows":[["1"]],"returned_rows":1,"duration_ms":5,"truncated":false}
```nested
````
## 问题 3
#### 1 tool `terminal`
````text
exit_code: 2
ERR {'code': 'FUNCTION_NOT_ALLOWED', 'message': 'no', 'retryable': False}
````
'''
        result = audit(text)
        self.assertEqual([q["question"] for q in result["questions"]], [2, 3])
        first, second = result["questions"]
        self.assertEqual(first["error_counts"], {"INTERNAL": 1})
        self.assertEqual(first["shell_masked_errors"], 1)
        self.assertEqual(first["parsed_query_results"], 1)
        self.assertEqual(first["responses"][0]["line"], 6)
        self.assertEqual(second["error_counts"], {"FUNCTION_NOT_ALLOWED": 1})
        self.assertEqual(second["shell_masked_errors"], 0)

    def test_assistant_copies_and_malformed_output_do_not_count_as_success(self):
        text = '''## 问题 1
#### 1 assistant
````text
{"query_id":"invented","rows":[]}
````
#### 2 tool `terminal`
````text
exit_code: 0
{"rows": [ cut off
{"query_id":"q_truncated","rows":[],"truncated":true,"truncation_reasons":["rows"]}
````
'''
        question = audit(text)["questions"][0]
        self.assertEqual(question["parsed_query_results"], 1)
        self.assertEqual(question["truncated_results"], 1)
        self.assertNotIn("rows", question["responses"][0])


if __name__ == "__main__":
    unittest.main()
