I want to add an experimental feature
In the config we can add a:
```
augment-backend:
  url: "http://cosmo.lan:8701"
  model: "cosmo-6000"
```

This LLM provider should behave almost exactly like the other backends
BUT it will not be in "normal" rotation / usage.

Instead of that we will do this
When the proxy recieves a request we should extract the user content
ie from this:
```
{"role": "user", "content": "Hello, Claude"}
```

Extract: "Hello, Claude"
(NOTE WE NEED TO SUPPORT ANTHROPIC AND OPENAI API)

Then we should send that message AS A NON TOOL CALLING message (normal chat)
Over to the augement-backend
BUT it needs to be wrapped in an outer prompt
This outer prompt will be stored in ./augmenter/backend_prompt.md
The user content should be appended to the prompt.md data
```
<augmenter_prompt>
<user_content>
```

When we recive the response from the augmenter-backend we should extract the content from the response JSON
Example anthropic response:
```
{
  "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "Hello guy!!!"
    }
  ],
```

And then we grab the "text" and concatenate the following before sending to the real backend:
- The orignal user content (in example above "Hello, Claude")
- The ./augmenter/request_prompt.md
- The response conten (in example above "Hello guy!!!"

Our goal is to always use a call to a very fast LLM to enrich the information flowing to the actual backend

Also add an option ` --log-augmented-request-text` when set we should log at INFO level
The new concatenated text
