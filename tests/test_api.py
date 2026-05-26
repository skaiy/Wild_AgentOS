"""
Test Suite for Items & Users API
==================================

Uses **pytest** with **FastAPI TestClient** (sync) to test all RESTful endpoints.

Covers:
- Health check (``/health``)
- Full CRUD for items (Create, Read, Update, Delete)
- Create and List for users
- The reset endpoint used to guarantee a clean state between runs
"""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from app.main import app
from app.routes import reset_store

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def client() -> TestClient:
    """Provide a synchronous HTTP client wired to the FastAPI app.

    ``TestClient`` internally uses ``httpx`` and handles the ASGI ↔ WSGI
    bridging transparently.
    """
    return TestClient(app)


@pytest.fixture(autouse=True)
def reset_store_before_test() -> None:
    """Reset the in-memory store before every test.

    This guarantees test isolation – no test will see data left behind by
    a previous test.
    """
    reset_store()


# ---------------------------------------------------------------------------
# Health check
# ---------------------------------------------------------------------------


def test_health_check(client: TestClient) -> None:
    """Verify that the ``/health`` endpoint returns a healthy status."""
    response = client.get("/health")
    assert response.status_code == 200
    assert response.json() == {"status": "healthy"}


# ---------------------------------------------------------------------------
# Item CRUD
# ---------------------------------------------------------------------------


def test_create_item(client: TestClient) -> None:
    """Create an item and verify the response contains an ID."""
    payload = {"name": "Test Item", "description": "A test", "price": 9.99}
    response = client.post("/api/items", json=payload)
    assert response.status_code == 201
    data = response.json()
    assert data["name"] == "Test Item"
    assert data["description"] == "A test"
    assert data["price"] == 9.99
    assert "id" in data


def test_list_items_empty(client: TestClient) -> None:
    """An empty store should return an empty list."""
    response = client.get("/api/items")
    assert response.status_code == 200
    assert response.json() == []


def test_list_items_non_empty(client: TestClient) -> None:
    """After creating one item the list should contain one entry."""
    client.post("/api/items", json={"name": "A", "price": 1.0})
    response = client.get("/api/items")
    assert response.status_code == 200
    data = response.json()
    assert len(data) == 1
    assert data[0]["name"] == "A"


def test_get_item(client: TestClient) -> None:
    """Retrieve a single item by its ID."""
    created = client.post("/api/items", json={"name": "Target", "price": 5.0})
    item_id = created.json()["id"]

    response = client.get(f"/api/items/{item_id}")
    assert response.status_code == 200
    data = response.json()
    assert data["id"] == item_id
    assert data["name"] == "Target"
    assert data["price"] == 5.0


def test_get_item_not_found(client: TestClient) -> None:
    """Fetching a non-existent ID should yield 404."""
    response = client.get("/api/items/nonexistent-id")
    assert response.status_code == 404
    assert response.json() == {"detail": "Item not found"}


def test_update_item(client: TestClient) -> None:
    """Update an item's name and price."""
    created = client.post("/api/items", json={"name": "Old", "price": 1.0})
    item_id = created.json()["id"]

    response = client.put(
        f"/api/items/{item_id}",
        json={"name": "Updated", "price": 2.0},
    )
    assert response.status_code == 200
    data = response.json()
    assert data["name"] == "Updated"
    assert data["price"] == 2.0
    # description should be preserved (it was not sent)
    assert data["description"] == ""


def test_update_item_not_found(client: TestClient) -> None:
    """Updating a non-existent item should yield 404."""
    response = client.put(
        "/api/items/nonexistent-id",
        json={"name": "Nope"},
    )
    assert response.status_code == 404
    assert response.json() == {"detail": "Item not found"}


def test_delete_item(client: TestClient) -> None:
    """Delete an item and confirm the store is then empty."""
    created = client.post("/api/items", json={"name": "ToDelete", "price": 0})
    item_id = created.json()["id"]

    response = client.delete(f"/api/items/{item_id}")
    assert response.status_code == 200
    assert response.json() == {"message": "Item deleted successfully"}

    # Verify it is gone
    get_resp = client.get(f"/api/items/{item_id}")
    assert get_resp.status_code == 404


def test_delete_item_not_found(client: TestClient) -> None:
    """Deleting a non-existent item should yield 404."""
    response = client.delete("/api/items/nonexistent-id")
    assert response.status_code == 404
    assert response.json() == {"detail": "Item not found"}


# ---------------------------------------------------------------------------
# User endpoints
# ---------------------------------------------------------------------------


def test_create_user(client: TestClient) -> None:
    """Create a user and verify the response contains an ID."""
    payload = {"username": "alice", "email": "alice@example.com", "role": "admin"}
    response = client.post("/api/users", json=payload)
    assert response.status_code == 201
    data = response.json()
    assert data["username"] == "alice"
    assert data["email"] == "alice@example.com"
    assert data["role"] == "admin"
    assert "id" in data


def test_list_users(client: TestClient) -> None:
    """List users – start empty, then add one."""
    # Initially empty
    resp1 = client.get("/api/users")
    assert resp1.status_code == 200
    assert resp1.json() == []

    # Create a user
    client.post("/api/users", json={"username": "bob", "email": "bob@ex.com"})

    resp2 = client.get("/api/users")
    assert resp2.status_code == 200
    data = resp2.json()
    assert len(data) == 1
    assert data[0]["username"] == "bob"


# ---------------------------------------------------------------------------
# Reset endpoint
# ---------------------------------------------------------------------------


def test_reset_store(client: TestClient) -> None:
    """Reset clears both items and users."""
    client.post("/api/items", json={"name": "X", "price": 1})
    client.post("/api/users", json={"username": "y", "email": "y@ex.com"})

    reset_resp = client.post("/api/reset")
    assert reset_resp.status_code == 200
    assert reset_resp.json() == {"message": "Store reset successfully"}

    # Both stores should now be empty
    assert client.get("/api/items").json() == []
    assert client.get("/api/users").json() == []
