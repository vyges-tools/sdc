create_clock -name clk -period 10 [get_ports clk]
set_input_delay 1 -clock clk [get_ports din]
set_false_path -from [get_cells s1] -to [get_pins s2/D]
set_multicycle_path 2 -setup -to [get_cells {s*}]
