package policy

import future.keywords.every
import future.keywords.if

default allow := false

allow if {
	every k0, v0 in data.reference {
		some k1
		endswith(k1, k0)
		match_value(v0, input[k1])
	}
}

match_value(reference_value, input_value) if {
	not is_array(reference_value)
	input_value == reference_value
}

match_value(reference_value, input_value) if {
	is_array(reference_value)
	array_include(reference_value, input_value)
}

array_include(reference_value_array, _) if {
	reference_value_array == []
}

array_include(reference_value_array, input_value) if {
	reference_value_array != []
	some i
	reference_value_array[i] == input_value
}
