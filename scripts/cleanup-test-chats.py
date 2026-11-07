"""Hide the "Native client test" meeting chats that live call tests leave behind.

Runs through the signed-in M365 web session (skill running-m365-web-session).
"""
import os
import sys
import urllib.parse

sys.path.insert(0, os.path.expanduser("~/.claude/skills/running-m365-web-session"))
from m365web.client import Client

SUBJECT = "Native client test"
GRAPH = "https://graph.microsoft.com/v1.0"
SCOPE = "Chat.ReadWrite"

with Client() as client:
    me = client.request("GET", f"{GRAPH}/me?$select=id", SCOPE)
    organization = client.request("GET", f"{GRAPH}/organization?$select=id", SCOPE)
    user = {"id": me["id"], "tenantId": organization["value"][0]["id"]}
    url = f"{GRAPH}/me/chats?$select=id,topic&$filter=topic eq '{SUBJECT}'&$top=50"
    chat_ids = []
    while url:
        page = client.request("GET", url, SCOPE)
        chat_ids += [chat["id"] for chat in page["value"] if chat.get("topic") == SUBJECT]
        url = page.get("@odata.nextLink")
    for chat_id in chat_ids:
        path = urllib.parse.quote(chat_id, safe="")
        client.request("POST", f"{GRAPH}/chats/{path}/hideForUser", SCOPE, body={"user": user})
    print(f"hid {len(chat_ids)} test meeting chat(s)")
