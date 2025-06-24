import json
import os
import random

from locust import HttpUser, task, between

class StressTestUser(HttpUser):
    host = os.getenv("LOCUST_HOST", "http://localhost:4554")
    wait_time = between(1, 5)  # Configurable delay between tasks


    def on_start(self):
        self.webhook = os.getenv("LOCUST_WEBHOOK", "AAAA")

    @task
    def get_time(self):
        body = '{"get_time": true}'
        self.client.get(f"/webhook/{self.webhook}", json=json.loads(body))

    @task
    def get_random_bytes(self):
        bytes_num = random.randint(1,100)
        body = f'{{"get_random_bytes": {bytes_num}}}'
        self.client.get(f"/webhook/{self.webhook}", json=json.loads(body))

    @task
    def use_cache(self):
        body = '{"use_cache": true}'
        self.client.get(f"/webhook/{self.webhook}", json=json.loads(body))
