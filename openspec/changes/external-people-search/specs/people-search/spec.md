# Spec Delta

## Purpose

Find people by name or email in search and when starting a chat, including people outside the signed-in organization.

## ADDED Requirements

### Requirement: Name search includes known people outside the organization

A name query in search and in the new-chat recipient field SHALL return people from the signed-in organization and people from other organizations who already appear in the people index. A partial name of a person who has never been contacted SHALL NOT be resolved from outside the organization.

#### Scenario: Known external person matches a name

- **WHEN** the user types the name of a person from another organization who is already in the people index
- **THEN** that person appears as a person result in search and in the new-chat recipient field

#### Scenario: Unknown external person does not match a partial name

- **WHEN** the user types a partial name that matches nobody in the organization and nobody in the people index
- **THEN** no person result is shown for that name

### Requirement: A complete email resolves a person outside the organization

When the query is a complete email address and the organization directory has no matching person, search and the new-chat recipient field SHALL show the person returned by the external lookup, including a federated work account. The match SHALL succeed when the returned address is the mail address or the user principal name. An incomplete address that contains `@` SHALL stay a directory search and SHALL NOT call the external lookup.

#### Scenario: Full email of someone outside the organization

- **WHEN** the user enters a complete email that is not in the organization directory and the external lookup returns that person
- **THEN** the person appears as a person result

#### Scenario: Address only on the user principal name

- **WHEN** the external lookup returns the queried email only as the user principal name
- **THEN** the person still appears as a person result

#### Scenario: Personal Microsoft account

- **WHEN** the external lookup returns one profile with an `8:live` MRI, no directory object id, and the queried email in `emailsInfo`
- **THEN** the person appears as a person result and keeps that `8:live` MRI as the id

#### Scenario: Directory miss does not hide the external person

- **WHEN** the directory lookup for a complete email returns no person or reports that the address is unknown
- **THEN** the external lookup still runs and its person is shown

#### Scenario: External person still shows when the directory fails

- **WHEN** the directory lookup for a complete email fails for a reason other than an unknown address and the external lookup returns that person
- **THEN** the person appears as a person result

#### Scenario: Directory failure without an external person is a failed search

- **WHEN** the directory lookup for a complete email fails for a reason other than an unknown address and the external lookup returns no matching person
- **THEN** the search is reported as failed and the empty result is not kept

#### Scenario: New chat looks up a complete email once

- **WHEN** the user types a complete email in the new-chat recipient field
- **THEN** the people-index suggestions and the directory lookup run together, and the external lookup runs at most once

#### Scenario: Incomplete email stays in the directory

- **WHEN** the query contains `@` but is not a complete email address
- **THEN** only directory results are shown

### Requirement: Person rows identify who the person is

A person result SHALL keep an id taken from the directory object id or, when that is absent, from an `8:orgid` MRI. A personal Microsoft account SHALL keep its full `8:live` MRI as the id. The row subtitle SHALL show the job title when present, otherwise the email, and SHALL include the organization name when one is known. Choosing the person SHALL open the existing 1:1 chat or create one.

#### Scenario: MRI supplies the id

- **WHEN** a person result has an `8:orgid` MRI and no directory object id
- **THEN** the result still appears and can be chosen

#### Scenario: Personal MRI supplies the id

- **WHEN** a person result has an `8:live` MRI and no directory object id
- **THEN** the result still appears and the id stays the full `8:live` MRI

#### Scenario: Subtitle shows organization

- **WHEN** a person result includes an organization name and no job title
- **THEN** the row shows the email and the organization name

#### Scenario: Choosing the person opens the chat

- **WHEN** the user chooses a person result
- **THEN** the existing 1:1 chat opens, or a new 1:1 chat is created
