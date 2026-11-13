# Linux Teams web scopes and chat fallback

On 2026-11-12 the signed-in Teams Chromium page supplied Graph User.ReadBasic.All and ChatMember.Read, but neither User.Read nor Chat.Read. GET /v1.0/me?$select=id,displayName,mail,userPrincipalName succeeded (200) using the already-granted User.ReadBasic.All token. A Graph chat-list request returned 403. Explicit refresh requests for User.Read/Chat.Read returned AADSTS65002, so no tenant permissions should be fabricated or bypassed.

The same page had an authorized IC3 Teams.AccessAsUser.All token. The Teams chat service returned 200 for conversation lists, thread metadata, and message history. Graph chat member listing returned 200 using ChatMember.Read. No credentials or message contents were recorded in these notes.

Use Graph when its chat scope is present. Otherwise use the existing Teams chat service token for chat listing and history, and retain Graph for members, profiles and already-granted message sending. All requests continue to enforce server permissions. Existing write formats remain unchanged.
