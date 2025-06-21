import json
import os

from locust import HttpUser, task, between

class StressTestUser(HttpUser):
    host = os.getenv("LOCUST_HOST", "http://localhost:4554")
    wait_time = between(1, 5)  # Configurable delay between tasks

    def on_start(self):
        # Load configurable request body from environment variable or use default
        body_str = os.getenv("LOCUST_JSON_BODY", '{"key": "value"}')
        try:
            self.json_body = json.loads(body_str)
        except json.JSONDecodeError:
            print("Invalid JSON in LOCUST_JSON_BODY, falling back to default.")
            self.json_body = {"key": "value"}

    @task
    def post_request_1(self):
        self.client.post("/webhook/LOADTEST1", json=self.json_body)

    @task
    def post_request_2(self):
        self.client.post("/webhook/LOADTEST2", json=self.json_body)

    @task
    def post_request_4(self):
        self.client.post("/webhook/LOADTEST4", json=self.json_body)
