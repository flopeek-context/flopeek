// SQLite adapter assembly for Last-Known-Good Protocol 1.0.
//
// The support, command, and query portions are included in one private
// implementation namespace so they share private adapter helpers without
// widening the public facade. Each portion has one bounded responsibility.

include!("lkg_protocol_support.rs");
include!("lkg_protocol_commands.rs");
include!("lkg_protocol_queries.rs");
