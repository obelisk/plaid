# Plaid load testing
The goal of these tests is to measure Plaid's performance when it is hit by a configurable amount of traffic. To execute these tests we use [Locust](https://locust.io/), a Python-based load testing framework.

## Running the load tests
In short, one needs to install the dependencies, run `locust` and target a running Plaid instance (local or remote). The preferred way to run these tests is in a virtual environment. Here is a breakdown of all the commands:

1. `python3 -m venv venv`
2. `. ./venv/bin/activate`
3. `pip install -U pip locust`
4. (From the folder where `locustfile.py` is located) `locust`
5. Press `Enter` or navigate to the provided URL to access Locust's web UI
6. Change the host if needed, to point it to Plaid's base URL (i.e., without `/webhook/...`). Ensure Plaid is running and reachable at this URL
7. Fill in the max number of "users" that will hit Plaid simultaneously, and the ramp-up rate
8. Start!

⚠️ Ensure that rules are properly configured for whatever test you want to run.

## Changing what load tests do
The behavior of test users is defined in `locustfile.py`.
For more information, refer to the original [documentation](https://docs.locust.io/en/stable/writing-a-locustfile.html) on how to write a `locustfile`.
In essence, it boils down to using the `client` object to send request to a given endpoint on the host.

## Increasing the load
For more intensive testing, one can
* Use the faster `FastHttpUser` that [ships](https://docs.locust.io/en/stable/increase-performance.html#increase-performance) with Locust
* Leverage Locust's [distributed load generation](https://docs.locust.io/en/stable/running-distributed.html)
