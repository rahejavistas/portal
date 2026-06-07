# Vistas File Server

A document sharing portal with role-based access control.

## Running the Application

Start the local stack (database, storage, server, and UI) with:

```bash
docker compose up --build
```

Stop with:
```bash
docker compose down -v
```

---

## Authentication

Authentication is stateless and uses JWT (JSON Web Tokens).

1. **Login Flow**: The client calls `/api/v1/login` with email and password. On success, the server returns a signed JWT.
2. **Session Persistence**: The client stores the JWT in `localStorage` under `vfs_token` and includes it in subsequent API requests as a bearer token (`Authorization: Bearer <token>`).
3. **Password Setup / Reset**:
   * Admin adds a new resident.
   * Resident requests a password setup link via `/api/v1/forgot-password`.
   * A single-use 32-character secure token is saved in the database with an expiration.
   * The user clicks the link and submits a new password to `/api/v1/reset-password`, which verifies the token and updates the password hash.

---

## Authorization

Role-based access controls are enforced on both the backend database queries and frontend navigation menus:

* **Resident**: Can view `PUBLIC` documents and `PRIVATE` documents explicitly mapped to their specific unit (Wing/Flat).
* **Committee**: Can view `PUBLIC` and `COMMITTEE_ONLY` documents.
* **Admin**: Has full access. Can list all documents/residents, create/update/delete documents, and add/update user roles.

---

## API Reference

All application endpoints are accessed via **POST** (except `/health` which uses **GET**).

<details>
<summary><b>Authentication & Password APIs</b></summary>

### POST `/api/v1/login`
Authenticates a user and returns a JWT.
* **Request Body:**
  ```json
  {
    "email": "resident@vistas.com",
    "password": "password"
  }
  ```
* **Response (200 OK):**
  ```json
  {
    "status": "success",
    "user": {
      "user_id": 1,
      "email": "resident@vistas.com",
      "name": "John Doe",
      "wing": "A",
      "flat_number": "101",
      "roles": ["Resident"]
    },
    "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
  }
  ```

### POST `/api/v1/forgot-password`
Generates a secure password setup / reset link for the given email.
* **Request Body:**
  ```json
  {
    "email": "resident@vistas.com"
  }
  ```
* **Response (200 OK):**
  ```json
  {
    "status": "success",
    "reset_link": "http://localhost:4000/reset-password?token=a1b2c3d4...",
    "message": "Password setup/reset link generated successfully"
  }
  ```

### POST `/api/v1/reset-password`
Completes password setup/reset using a valid database token.
* **Request Body:**
  ```json
  {
    "token": "a1b2c3d4...",
    "password": "newpassword123"
  }
  ```
* **Response (200 OK):**
  ```json
  {
    "status": "success",
    "message": "Password updated successfully"
  }
  ```

</details>

<details>
<summary><b>Resident Document APIs</b></summary>

### POST `/api/v1/documents`
Lists visible active documents for the authenticated user.
* **Request Headers:** `Authorization: Bearer <token>`
* **Response (200 OK):**
  ```json
  [
    {
      "document": {
        "document_id": "8f8e8d8c...",
        "title": "Meeting Minutes May 2026",
        "description": "General body minutes",
        "category": "MEETING_MINUTES",
        "document_type": "GENERAL_BODY_MEETING",
        "access_level": "PUBLIC",
        "is_deleted": false
      },
      "metadata": {
        "subcategory": "General",
        "tags": ["meeting", "minutes"],
        "document_date": "2026-05-19"
      }
    }
  ]
  ```

### POST `/api/v1/documents/:id`
Retrieves a specific document.
* **Request Headers:** `Authorization: Bearer <token>`
* **Response (200 OK):**
  ```json
  {
    "document": {
      "document_id": "8f8e8d8c...",
      "title": "Meeting Minutes May 2026"
    },
    "metadata": {}
  }
  ```

</details>

<details>
<summary><b>Admin Users APIs</b></summary>

### POST `/api/v1/admin/users/list`
Lists all residents in the system.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Response (200 OK):**
  ```json
  [
    {
      "user_id": 1,
      "email": "resident@vistas.com",
      "name": "John Doe",
      "wing": "A",
      "flat_number": "101",
      "roles": ["Resident"]
    }
  ]
  ```

### POST `/api/v1/admin/users/create`
Adds a new resident to the system.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Request Body:**
  ```json
  {
    "email": "newuser@vistas.com",
    "name": "Jane Smith",
    "wing": "B",
    "flat_number": "202"
  }
  ```
* **Response (201 Created):**
  ```json
  {
    "user_id": 2,
    "email": "newuser@vistas.com",
    "name": "Jane Smith",
    "wing": "B",
    "flat_number": "202",
    "roles": ["Resident"]
  }
  ```

### POST `/api/v1/admin/users/roles`
Updates a resident's access role (Resident, Committee, Admin).
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Request Body:**
  ```json
  {
    "user_id": 2,
    "target_role": "Committee"
  }
  ```
* **Response (200 OK):**
  ```json
  {
    "status": "success",
    "message": "User role updated successfully."
  }
  ```

</details>

<details>
<summary><b>Admin Document APIs</b></summary>

### POST `/api/v1/admin/documents/list`
Lists all documents (including soft-deleted ones) with no filters.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Response (200 OK):** List of document objects.

### POST `/api/v1/admin/documents/create`
Creates a new document, registering metadata and S3 references.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Request Body:**
  ```json
  {
    "title": "Vendor Contract",
    "description": "Paint contract",
    "category": "OPERATIONAL",
    "document_type": "OPERATIONAL_VENDOR_CONTRACT",
    "access_level": "PUBLIC",
    "flat": null,
    "metadata": {
      "subcategory": "Paint",
      "tags": ["contract", "vendor"],
      "document_date": "2026-05-19"
    }
  }
  ```
* **Response (201 Created):** Document object with created metadata.

### POST `/api/v1/admin/documents/:id/get`
Fetches details of any specific document (including deleted ones).
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Response (200 OK):** Document details.

### POST `/api/v1/admin/documents/:id/update`
Updates an existing document's properties.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Request Body:** Same schema as document creation.
* **Response (200 OK):** Updated document details.

### POST `/api/v1/admin/documents/:id/delete`
Toggles the soft-deleted state of a document.
* **Request Headers:** `Authorization: Bearer <token>` (Admin required)
* **Response (204 No Content)**

</details>