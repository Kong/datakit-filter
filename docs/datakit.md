# DataKit

DataKit is effectively a dataflow language: a filter configuration specifies a directed graph of
operations to be performed, based on their data dependencies.

## The data model

The data types are based on those of [serde-json], so representable value types are:

* Null
* Boolean
* Number
* String
* Array (a vector of values)
* Object (a map from strings to values)

## The execution model

Each node triggers at most once.

A node only triggers when all its inputs are available.

## Node types

The following node types are implemented:

* `call`: an HTTP dispatch call
* `template`: application of a string template
* `response`: trigger a direct response, rather than forwarding a proxied response

## Implicit nodes

DataKit defines a number of implicit nodes that can be used as inputs or outputs without being
explicitly declared. These reserved node names cannot be used for user-defined nodes. These are:

**Name**                    |  **Supported**     |  **Usage**     |  **Description**
---------------------------:|:------------------:|:--------------:|:------------------
`request_headers`           | :heavy_check_mark: | as input only  | headers from the incoming request
`request_body`              | :heavy_check_mark: | as input only  | body of the incoming request
`service_request_headers`   | :x:                | as output only | headers to be sent to the service being proxied to
`service_request_body`      | :heavy_check_mark: | as output only | body to be sent to the service being proxied to
`service_response_headers`  | :heavy_check_mark: | as input only  | headers from the response sent by the service being proxied to
`service_response_body`     | :heavy_check_mark: | as input only  | body of the response sent by the service being proxied to
`response_headers`          | :x:                | as output only | headers to be sent as a response to the incoming request
`response_body`             | :heavy_check_mark: | as output only | body to be sent as a response to the incoming request

The `_headers` nodes produce maps from header names to their values.
Keys are header names are normalized to lowercase.
Values are always an array of strings, even when there is a single header value.

The `_body` nodes produce either raw strings of JSON objects, depending on their corresponding
`Content-Type` values.

[serde-json]: https://docs.rs/serde_json/latest/serde_json/
